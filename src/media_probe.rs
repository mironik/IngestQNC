use std::{
    collections::HashSet,
    env,
    error::Error,
    ffi::{OsStr, OsString},
    fmt, io,
    path::{Component, Path, PathBuf},
    process::Command,
};

use rusqlite::{params, Connection};
use serde_json::Value;

use crate::db::{self, ClipCreationTimestampUpdate, MediaProbeSnapshotRecord};

pub const PROBE_VERSION: &str = "ffprobe-full-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRequest {
    pub source_identity: String,
    pub operator_access_root: PathBuf,
    pub selected_clip_ids: Vec<String>,
    pub probed_at: String,
}

impl ProbeRequest {
    pub fn new(
        source_identity: impl Into<String>,
        operator_access_root: impl Into<PathBuf>,
        selected_clip_ids: Vec<String>,
        probed_at: impl Into<String>,
    ) -> Self {
        Self {
            source_identity: source_identity.into(),
            operator_access_root: operator_access_root.into(),
            selected_clip_ids,
            probed_at: probed_at.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    pub source_identity: String,
    pub requested_clips: usize,
    pub candidate_clips: usize,
    pub probes_ok: usize,
    pub probes_error: usize,
    pub probes_skipped: usize,
}

pub fn probe_source_clips(
    content_db: &Connection,
    request: ProbeRequest,
) -> Result<ProbeReport, ProbeError> {
    let binary = ffprobe_binary();
    probe_source_clips_with_runner(content_db, request, |path| run_ffprobe(&binary, path))
}

fn ffprobe_binary() -> OsString {
    env::var_os("INGESTQNC_FFPROBE").unwrap_or_else(|| OsString::from("ffprobe"))
}

fn probe_source_clips_with_runner(
    content_db: &Connection,
    request: ProbeRequest,
    mut runner: impl FnMut(&Path) -> Result<ProbeCommandOutput, ProbeError>,
) -> Result<ProbeReport, ProbeError> {
    let source_identity = request.source_identity.trim();
    if source_identity.is_empty() {
        return Err(ProbeError::EmptySourceIdentity);
    }
    if !db::content_source_identity_exists(content_db, source_identity)? {
        return Err(ProbeError::SourceContentNotInitialized(
            source_identity.to_owned(),
        ));
    }

    let selected: HashSet<String> = request
        .selected_clip_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    let candidates = load_probe_candidates(content_db, source_identity, &selected)?;

    let mut report = ProbeReport {
        source_identity: source_identity.to_owned(),
        requested_clips: selected.len(),
        candidate_clips: candidates.len(),
        probes_ok: 0,
        probes_error: 0,
        probes_skipped: selected.len().saturating_sub(candidates.len()),
    };

    for candidate in candidates {
        let media_path =
            runtime_media_path(&request.operator_access_root, &candidate.relative_path)?;
        let output = runner(&media_path)?;
        let (snapshot, timestamp_update) =
            snapshot_from_output(&candidate, &output, &request.probed_at)?;
        db::upsert_media_probe_snapshot(content_db, &snapshot)?;
        if let Some(timestamp_update) = timestamp_update {
            db::update_clip_creation_timestamp(content_db, &timestamp_update)?;
        }
        if snapshot.status == "ok" {
            report.probes_ok += 1;
        } else {
            report.probes_error += 1;
        }
    }

    Ok(report)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeClipCandidate {
    clip_id: String,
    relative_path: String,
}

fn load_probe_candidates(
    content_db: &Connection,
    source_identity: &str,
    selected_clip_ids: &HashSet<String>,
) -> Result<Vec<ProbeClipCandidate>, ProbeError> {
    let mut stmt = content_db.prepare(
        "
        SELECT clip_id, relative_path
        FROM ingestqnc_clips
        WHERE source_identity = ?1 AND deleted_at IS NULL
        ORDER BY relative_path COLLATE NOCASE
        ",
    )?;
    let rows = stmt.query_map(params![source_identity], |row| {
        Ok(ProbeClipCandidate {
            clip_id: row.get(0)?,
            relative_path: row.get(1)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        let candidate = row?;
        if selected_clip_ids.contains(&candidate.clip_id) {
            out.push(candidate);
        }
    }
    Ok(out)
}

fn runtime_media_path(root: &Path, relative_path: &str) -> Result<PathBuf, ProbeError> {
    let mut out = root.to_path_buf();
    for segment in relative_path.split('/') {
        let segment = segment.trim();
        if segment.is_empty() || segment.contains('\\') {
            return Err(ProbeError::InvalidRelativePath(relative_path.to_owned()));
        }
        let path = Path::new(segment);
        if path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
        {
            return Err(ProbeError::InvalidRelativePath(relative_path.to_owned()));
        }
        out.push(segment);
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeCommandOutput {
    success: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_ffprobe(binary: &OsStr, media_path: &Path) -> Result<ProbeCommandOutput, ProbeError> {
    let output = Command::new(binary)
        .arg("-v")
        .arg("error")
        .arg("-print_format")
        .arg("json")
        .arg("-show_format")
        .arg("-show_streams")
        .arg("-show_chapters")
        .arg("-show_programs")
        .arg(media_path)
        .output()
        .map_err(ProbeError::CommandIo)?;

    Ok(ProbeCommandOutput {
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn snapshot_from_output(
    candidate: &ProbeClipCandidate,
    output: &ProbeCommandOutput,
    probed_at: &str,
) -> Result<
    (
        MediaProbeSnapshotRecord,
        Option<ClipCreationTimestampUpdate>,
    ),
    ProbeError,
> {
    let probe_id = stable_probe_id(&candidate.clip_id);
    if !output.success {
        let raw_probe_json = error_probe_json(output, "ffprobe exited with an error")?;
        return Ok((
            MediaProbeSnapshotRecord {
                probe_id,
                clip_id: candidate.clip_id.clone(),
                probe_version: PROBE_VERSION.into(),
                probed_at: probed_at.into(),
                status: "error".into(),
                duration_sec: None,
                video_codec: None,
                audio_codec: None,
                width: None,
                height: None,
                fps_num: None,
                fps_den: None,
                raw_probe_json,
                error: Some(short_error(&output.stderr, "ffprobe exited with an error")),
            },
            None,
        ));
    }

    let raw: Value = match serde_json::from_str(&output.stdout) {
        Ok(raw) => raw,
        Err(error) => {
            let raw_probe_json =
                error_probe_json(output, &format!("ffprobe JSON parse failed: {error}"))?;
            return Ok((
                MediaProbeSnapshotRecord {
                    probe_id,
                    clip_id: candidate.clip_id.clone(),
                    probe_version: PROBE_VERSION.into(),
                    probed_at: probed_at.into(),
                    status: "error".into(),
                    duration_sec: None,
                    video_codec: None,
                    audio_codec: None,
                    width: None,
                    height: None,
                    fps_num: None,
                    fps_den: None,
                    raw_probe_json,
                    error: Some(format!("ffprobe JSON parse failed: {error}")),
                },
                None,
            ));
        }
    };
    let summary = summarize_probe(&raw);
    let timestamp_update =
        summary
            .creation_timestamp
            .as_ref()
            .map(|timestamp| ClipCreationTimestampUpdate {
                clip_id: candidate.clip_id.clone(),
                clip_created_at: timestamp.value.clone(),
                clip_created_at_source: timestamp.source.clone(),
                clip_created_at_offset: timestamp.offset.clone(),
                timestamp_evidence_json: serde_json::json!({
                    "schema_version": 1,
                    "source": "media_probe",
                    "probe_version": PROBE_VERSION,
                    "tag_path": timestamp.tag_path,
                    "tag_key": timestamp.tag_key,
                    "clip_created_at": timestamp.value,
                    "clip_created_at_source": timestamp.source,
                    "clip_created_at_offset": timestamp.offset,
                    "note": "embedded media timestamp from ffprobe overrides filesystem fallback"
                })
                .to_string(),
            });

    Ok((
        MediaProbeSnapshotRecord {
            probe_id,
            clip_id: candidate.clip_id.clone(),
            probe_version: PROBE_VERSION.into(),
            probed_at: probed_at.into(),
            status: "ok".into(),
            duration_sec: summary.duration_sec,
            video_codec: summary.video_codec,
            audio_codec: summary.audio_codec,
            width: summary.width,
            height: summary.height,
            fps_num: summary.fps.map(|fps| fps.0),
            fps_den: summary.fps.map(|fps| fps.1),
            raw_probe_json: output.stdout.clone(),
            error: None,
        },
        timestamp_update,
    ))
}

#[derive(Debug, Clone, Default, PartialEq)]
struct ProbeSummary {
    duration_sec: Option<f64>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    fps: Option<(i64, i64)>,
    creation_timestamp: Option<CreationTimestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CreationTimestamp {
    value: String,
    source: String,
    offset: Option<String>,
    tag_path: String,
    tag_key: String,
}

fn summarize_probe(raw: &Value) -> ProbeSummary {
    let streams = raw
        .get("streams")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let video = streams
        .iter()
        .find(|stream| stream.get("codec_type").and_then(Value::as_str) == Some("video"));
    let audio = streams
        .iter()
        .find(|stream| stream.get("codec_type").and_then(Value::as_str) == Some("audio"));

    ProbeSummary {
        duration_sec: raw
            .pointer("/format/duration")
            .and_then(Value::as_str)
            .and_then(parse_f64_text)
            .or_else(|| {
                streams
                    .iter()
                    .filter_map(|stream| {
                        stream
                            .get("duration")
                            .and_then(Value::as_str)
                            .and_then(parse_f64_text)
                    })
                    .max_by(|a, b| a.total_cmp(b))
            }),
        video_codec: video
            .and_then(|stream| stream.get("codec_name"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        audio_codec: audio
            .and_then(|stream| stream.get("codec_name"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        width: video
            .and_then(|stream| stream.get("width"))
            .and_then(Value::as_i64),
        height: video
            .and_then(|stream| stream.get("height"))
            .and_then(Value::as_i64),
        fps: video.and_then(|stream| {
            stream
                .get("avg_frame_rate")
                .and_then(Value::as_str)
                .and_then(parse_rate)
                .or_else(|| {
                    stream
                        .get("r_frame_rate")
                        .and_then(Value::as_str)
                        .and_then(parse_rate)
                })
        }),
        creation_timestamp: find_creation_timestamp(raw),
    }
}

fn find_creation_timestamp(raw: &Value) -> Option<CreationTimestamp> {
    let format_tags = raw.pointer("/format/tags");
    if let Some(timestamp) = find_timestamp_in_tags(format_tags, "format.tags") {
        return Some(timestamp);
    }

    raw.get("streams")
        .and_then(Value::as_array)?
        .iter()
        .enumerate()
        .find_map(|(index, stream)| {
            find_timestamp_in_tags(stream.get("tags"), &format!("streams[{index}].tags"))
        })
}

fn find_timestamp_in_tags(tags: Option<&Value>, tag_path: &str) -> Option<CreationTimestamp> {
    let tags = tags?.as_object()?;
    let camera_keys = [
        "com.apple.quicktime.creationdate",
        "creationdate",
        "creation_date",
        "date",
    ];
    for key in camera_keys {
        if let Some((actual_key, value)) = tag_value_case_insensitive(tags, key) {
            return Some(CreationTimestamp {
                value: value.to_owned(),
                source: "embedded_camera".into(),
                offset: timestamp_offset(&value),
                tag_path: tag_path.into(),
                tag_key: actual_key,
            });
        }
    }
    if let Some((actual_key, value)) = tag_value_case_insensitive(tags, "creation_time") {
        return Some(CreationTimestamp {
            value: value.to_owned(),
            source: "embedded_container".into(),
            offset: timestamp_offset(&value),
            tag_path: tag_path.into(),
            tag_key: actual_key,
        });
    }
    None
}

fn tag_value_case_insensitive(
    tags: &serde_json::Map<String, Value>,
    wanted: &str,
) -> Option<(String, String)> {
    tags.iter().find_map(|(key, value)| {
        key.eq_ignore_ascii_case(wanted)
            .then(|| value.as_str().map(|value| (key.clone(), value.to_owned())))
            .flatten()
    })
}

fn timestamp_offset(value: &str) -> Option<String> {
    let value = value.trim();
    if value.ends_with('Z') || value.ends_with('z') {
        return Some("+00:00".into());
    }
    if value.len() >= 6 {
        let suffix = &value[value.len() - 6..];
        let bytes = suffix.as_bytes();
        if matches!(bytes.first(), Some(b'+') | Some(b'-'))
            && bytes.get(3) == Some(&b':')
            && bytes[1..3].iter().all(u8::is_ascii_digit)
            && bytes[4..6].iter().all(u8::is_ascii_digit)
        {
            return Some(suffix.into());
        }
    }
    None
}

fn parse_f64_text(value: &str) -> Option<f64> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn parse_rate(value: &str) -> Option<(i64, i64)> {
    let (num, den) = value.trim().split_once('/')?;
    let num = num.parse::<i64>().ok()?;
    let den = den.parse::<i64>().ok()?;
    (num > 0 && den > 0).then_some((num, den))
}

fn stable_probe_id(clip_id: &str) -> String {
    format!("probe:{clip_id}:{PROBE_VERSION}")
}

fn error_probe_json(output: &ProbeCommandOutput, message: &str) -> Result<String, ProbeError> {
    Ok(serde_json::json!({
        "schema_version": 1,
        "probe_version": PROBE_VERSION,
        "status": "error",
        "message": message,
        "exit_code": output.exit_code,
        "stdout": output.stdout,
        "stderr": output.stderr
    })
    .to_string())
}

fn short_error(stderr: &str, fallback: &str) -> String {
    let text = stderr.trim();
    let text = if text.is_empty() { fallback } else { text };
    const MAX: usize = 500;
    if text.chars().count() <= MAX {
        text.to_owned()
    } else {
        let mut out = text.chars().take(MAX).collect::<String>();
        out.push_str("...");
        out
    }
}

#[derive(Debug)]
pub enum ProbeError {
    EmptySourceIdentity,
    SourceContentNotInitialized(String),
    InvalidRelativePath(String),
    CommandIo(io::Error),
    Database(rusqlite::Error),
    Json(serde_json::Error),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySourceIdentity => write!(f, "source identity is empty"),
            Self::SourceContentNotInitialized(source_identity) => write!(
                f,
                "content database is not initialized for source identity: {source_identity}"
            ),
            Self::InvalidRelativePath(path) => {
                write!(f, "probe relative path is not OS-neutral: {path}")
            }
            Self::CommandIo(error) => write!(f, "ffprobe command failed: {error}"),
            Self::Database(error) => write!(f, "probe database write failed: {error}"),
            Self::Json(error) => write!(f, "probe JSON failed: {error}"),
        }
    }
}

impl Error for ProbeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CommandIo(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::EmptySourceIdentity
            | Self::SourceContentNotInitialized(_)
            | Self::InvalidRelativePath(_) => None,
        }
    }
}

impl From<rusqlite::Error> for ProbeError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

impl From<serde_json::Error> for ProbeError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;
    use crate::db::{ClipDiscoveryRecord, SourceIdentityRecord};

    const SOURCE_IDENTITY: &str = "card:test:probe-sn-001";
    const SEEN_AT: &str = "2026-09-03T10:00:00Z";
    const PROBED_AT: &str = "2026-09-03T10:05:00Z";

    #[test]
    fn summarize_probe_extracts_video_audio_fps_and_camera_timestamp() {
        let raw: Value = serde_json::from_str(SAMPLE_FFPROBE_JSON).unwrap();

        let summary = summarize_probe(&raw);

        assert_eq!(summary.duration_sec, Some(12.24));
        assert_eq!(summary.video_codec.as_deref(), Some("h264"));
        assert_eq!(summary.audio_codec.as_deref(), Some("aac"));
        assert_eq!(summary.width, Some(1920));
        assert_eq!(summary.height, Some(1080));
        assert_eq!(summary.fps, Some((30000, 1001)));
        let timestamp = summary.creation_timestamp.unwrap();
        assert_eq!(timestamp.value, "2026-09-03T12:14:22+02:00");
        assert_eq!(timestamp.source, "embedded_camera");
        assert_eq!(timestamp.offset.as_deref(), Some("+02:00"));
    }

    #[test]
    fn probe_source_clips_writes_snapshot_and_embedded_creation_time() {
        let content_db = initialized_content_db();
        let root = unique_temp_root("probe-source");
        let media_path = root.join("DCIM").join("A001_C001.MOV");
        write_file(media_path.clone(), b"media bytes");
        let clip_id = insert_clip(&content_db);

        let report = probe_source_clips_with_runner(
            &content_db,
            ProbeRequest::new(SOURCE_IDENTITY, &root, vec![clip_id.clone()], PROBED_AT),
            |path| {
                assert_eq!(path, media_path.as_path());
                Ok(ProbeCommandOutput {
                    success: true,
                    exit_code: Some(0),
                    stdout: SAMPLE_FFPROBE_JSON.into(),
                    stderr: String::new(),
                })
            },
        )
        .unwrap();

        assert_eq!(report.requested_clips, 1);
        assert_eq!(report.candidate_clips, 1);
        assert_eq!(report.probes_ok, 1);
        assert_eq!(report.probes_error, 0);

        let row: (
            String,
            Option<f64>,
            Option<String>,
            Option<i64>,
            Option<i64>,
        ) = content_db
            .query_row(
                "
                SELECT status, duration_sec, video_codec, width, height
                FROM ingestqnc_media_probe_snapshots
                WHERE clip_id = ?1
                ",
                params![clip_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, "ok");
        assert_eq!(row.1, Some(12.24));
        assert_eq!(row.2.as_deref(), Some("h264"));
        assert_eq!(row.3, Some(1920));
        assert_eq!(row.4, Some(1080));

        let timestamp: (Option<String>, String, Option<String>) = content_db
            .query_row(
                "
                SELECT clip_created_at, clip_created_at_source, clip_created_at_offset
                FROM ingestqnc_clips
                WHERE clip_id = ?1
                ",
                params![clip_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(timestamp.0.as_deref(), Some("2026-09-03T12:14:22+02:00"));
        assert_eq!(timestamp.1, "embedded_camera");
        assert_eq!(timestamp.2.as_deref(), Some("+02:00"));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn failed_probe_writes_error_snapshot_without_stopping_other_rows() {
        let content_db = initialized_content_db();
        let root = unique_temp_root("probe-error");
        write_file(root.join("DCIM").join("A001_C001.MOV"), b"media bytes");
        let clip_id = insert_clip(&content_db);

        let report = probe_source_clips_with_runner(
            &content_db,
            ProbeRequest::new(SOURCE_IDENTITY, &root, vec![clip_id.clone()], PROBED_AT),
            |_path| {
                Ok(ProbeCommandOutput {
                    success: false,
                    exit_code: Some(1),
                    stdout: String::new(),
                    stderr: "invalid data".into(),
                })
            },
        )
        .unwrap();

        assert_eq!(report.probes_ok, 0);
        assert_eq!(report.probes_error, 1);
        let row: (String, Option<String>, String) = content_db
            .query_row(
                "
                SELECT status, error, raw_probe_json
                FROM ingestqnc_media_probe_snapshots
                WHERE clip_id = ?1
                ",
                params![clip_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row.0, "error");
        assert_eq!(row.1.as_deref(), Some("invalid data"));
        assert!(row.2.contains("invalid data"));

        fs::remove_dir_all(root).ok();
    }

    fn initialized_content_db() -> Connection {
        let content_db = db::open_content_in_memory().unwrap();
        db::upsert_content_source_identity(
            &content_db,
            &SourceIdentityRecord {
                source_identity: SOURCE_IDENTITY.into(),
                source_kind: "local_card".into(),
                display_name: "Probe Card".into(),
                transport_uri: "qnc+local://localhost/card/probe-sn-001".into(),
                identity_evidence_json: "{}".into(),
                fallback_fingerprint: None,
                seen_at: SEEN_AT.into(),
            },
        )
        .unwrap();
        content_db
    }

    fn insert_clip(content_db: &Connection) -> String {
        db::upsert_clip(
            content_db,
            &ClipDiscoveryRecord {
                source_identity: SOURCE_IDENTITY.into(),
                clip_fingerprint: "fingerprint-001".into(),
                relative_path: "DCIM/A001_C001.MOV".into(),
                original_name: "A001_C001.MOV".into(),
                extension: Some("mov".into()),
                poster_relative_path: None,
                poster_source: None,
                file_size_bytes: Some(10),
                clip_created_at: Some("2026-09-03T09:00:00Z".into()),
                clip_created_at_source: "filesystem_created".into(),
                clip_created_at_offset: Some("+00:00".into()),
                timestamp_evidence_json: "{}".into(),
                seen_at: SEEN_AT.into(),
            },
        )
        .unwrap()
        .clip_id
    }

    fn unique_temp_root(kind: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ingestqnc-{kind}-{nanos}"))
    }

    fn write_file(path: PathBuf, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    const SAMPLE_FFPROBE_JSON: &str = r#"{
        "streams": [
            {
                "codec_type": "video",
                "codec_name": "h264",
                "width": 1920,
                "height": 1080,
                "avg_frame_rate": "30000/1001",
                "tags": {
                    "creation_time": "2026-09-03T10:14:22Z"
                }
            },
            {
                "codec_type": "audio",
                "codec_name": "aac"
            }
        ],
        "format": {
            "duration": "12.240000",
            "tags": {
                "com.apple.quicktime.creationdate": "2026-09-03T12:14:22+02:00"
            }
        }
    }"#;
}
