use std::{
    collections::HashMap,
    error::Error,
    fmt, fs,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::db::{self, ClipDiscoveryRecord, UpsertKind};

const FINGERPRINT_VERSION: &str = "content-sample-sha256-v1";
const FINGERPRINT_SAMPLE_BYTES: u64 = 1024 * 1024;
const POSTER_EXTENSIONS: &[&str] = &["thm", "jpg", "jpeg"];

pub const MEDIA_EXTENSIONS: &[&str] = &[
    "mov", "mp4", "m4v", "mxf", "mts", "m2ts", "avi", "mkv", "r3d", "braw", "ari", "crm", "cine",
    "wav", "aif", "aiff", "flac",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRequest {
    pub source_identity: String,
    pub operator_access_root: PathBuf,
    pub seen_at: String,
}

impl ScanRequest {
    pub fn new(
        source_identity: impl Into<String>,
        operator_access_root: impl Into<PathBuf>,
        seen_at: impl Into<String>,
    ) -> Self {
        Self {
            source_identity: source_identity.into(),
            operator_access_root: operator_access_root.into(),
            seen_at: seen_at.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanReport {
    pub source_identity: String,
    pub files_seen: usize,
    pub media_files_seen: usize,
    pub clips_inserted: usize,
    pub clips_updated: usize,
    pub skipped_non_media: usize,
}

pub fn scan_source(
    registry: &Connection,
    content_db: &Connection,
    request: ScanRequest,
) -> Result<ScanReport, ScanError> {
    let source_identity = request.source_identity.trim();
    if source_identity.is_empty() {
        return Err(ScanError::EmptySourceIdentity);
    }
    if !db::source_identity_exists(registry, source_identity)? {
        return Err(ScanError::UnknownSourceIdentity(source_identity.to_owned()));
    }
    if !db::content_source_identity_exists(content_db, source_identity)? {
        return Err(ScanError::SourceContentNotInitialized(
            source_identity.to_owned(),
        ));
    }

    let root_metadata = fs::metadata(&request.operator_access_root)?;
    if !root_metadata.is_dir() {
        return Err(ScanError::RootIsNotDirectory {
            root: request.operator_access_root,
        });
    }

    let mut report = ScanReport {
        source_identity: source_identity.to_owned(),
        files_seen: 0,
        media_files_seen: 0,
        clips_inserted: 0,
        clips_updated: 0,
        skipped_non_media: 0,
    };
    let poster_index = collect_poster_candidates(&request.operator_access_root)?;

    scan_directory(
        content_db,
        &request.operator_access_root,
        &request.operator_access_root,
        &poster_index,
        source_identity,
        &request.seen_at,
        &mut report,
    )?;

    Ok(report)
}

fn scan_directory(
    conn: &Connection,
    root: &Path,
    directory: &Path,
    poster_index: &PosterIndex,
    source_identity: &str,
    seen_at: &str,
    report: &mut ScanReport,
) -> Result<(), ScanError> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase());

    for entry in entries {
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            scan_directory(
                conn,
                root,
                &path,
                poster_index,
                source_identity,
                seen_at,
                report,
            )?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        report.files_seen += 1;
        let extension = normalized_extension(&path);
        if !is_media_extension(extension.as_deref()) {
            report.skipped_non_media += 1;
            continue;
        }

        report.media_files_seen += 1;
        let record = clip_record_for_file(
            &path,
            root,
            source_identity,
            extension,
            poster_index,
            seen_at,
        )?;
        match db::upsert_clip(conn, &record)?.kind {
            UpsertKind::Inserted => report.clips_inserted += 1,
            UpsertKind::Updated => report.clips_updated += 1,
        }
    }

    Ok(())
}

fn clip_record_for_file(
    path: &Path,
    root: &Path,
    source_identity: &str,
    extension: Option<String>,
    poster_index: &PosterIndex,
    seen_at: &str,
) -> Result<ClipDiscoveryRecord, ScanError> {
    let metadata = fs::metadata(path)?;
    let relative_path = relative_transport_path(root, path)?;
    let original_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ScanError::NonUnicodePath(path.to_path_buf()))?
        .to_owned();
    let file_size_bytes = i64::try_from(metadata.len()).ok();
    let timestamp = timestamp_evidence(&metadata)?;
    let poster = find_poster_for_media(path, poster_index);

    Ok(ClipDiscoveryRecord {
        source_identity: source_identity.to_owned(),
        clip_fingerprint: clip_fingerprint(path, metadata.len())?,
        relative_path,
        original_name,
        extension,
        poster_relative_path: poster.as_ref().map(|poster| poster.relative_path.clone()),
        poster_source: poster.as_ref().map(|poster| poster.source.clone()),
        file_size_bytes,
        clip_created_at: timestamp.clip_created_at,
        clip_created_at_source: timestamp.clip_created_at_source,
        clip_created_at_offset: None,
        timestamp_evidence_json: timestamp.timestamp_evidence_json,
        seen_at: seen_at.to_owned(),
    })
}

fn normalized_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(|extension| extension.to_ascii_lowercase())
}

fn is_media_extension(extension: Option<&str>) -> bool {
    extension
        .map(|extension| MEDIA_EXTENSIONS.contains(&extension))
        .unwrap_or(false)
}

fn is_poster_extension(extension: Option<&str>) -> bool {
    extension
        .map(|extension| POSTER_EXTENSIONS.contains(&extension))
        .unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PosterCandidate {
    relative_path: String,
    source: String,
    same_directory: bool,
}

type PosterIndex = HashMap<String, Vec<PosterCandidate>>;

fn collect_poster_candidates(root: &Path) -> Result<PosterIndex, ScanError> {
    let mut index = PosterIndex::new();
    collect_poster_candidates_in(root, root, &mut index)?;
    for candidates in index.values_mut() {
        candidates.sort_by_key(poster_rank);
    }
    Ok(index)
}

fn collect_poster_candidates_in(
    root: &Path,
    directory: &Path,
    index: &mut PosterIndex,
) -> Result<(), ScanError> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase());

    for entry in entries {
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_poster_candidates_in(root, &path, index)?;
            continue;
        }
        if !file_type.is_file() || !is_poster_extension(normalized_extension(&path).as_deref()) {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let key = poster_match_key(stem);
        if key.is_empty() {
            continue;
        }
        let relative_path = relative_transport_path(root, &path)?;
        let source = match normalized_extension(&path).as_deref() {
            Some("thm") => "card_thm",
            Some("jpg") | Some("jpeg") => "card_jpg",
            _ => "card_poster",
        }
        .to_owned();
        index.entry(key).or_default().push(PosterCandidate {
            relative_path,
            source,
            same_directory: false,
        });
    }

    Ok(())
}

fn find_poster_for_media(path: &Path, poster_index: &PosterIndex) -> Option<PosterCandidate> {
    let stem = path.file_stem()?.to_str()?;
    let key = poster_match_key(stem);
    if key.is_empty() {
        return None;
    }

    poster_index.get(&key).and_then(|candidates| {
        candidates
            .iter()
            .map(|candidate| {
                let mut candidate = candidate.clone();
                candidate.same_directory = poster_same_directory(path, &candidate.relative_path);
                candidate
            })
            .min_by_key(poster_rank)
    })
}

fn poster_rank(candidate: &PosterCandidate) -> (u8, u8, String) {
    let ext_rank = if candidate.source == "card_thm" { 0 } else { 1 };
    let dir_rank = if candidate.same_directory { 0 } else { 1 };
    (ext_rank, dir_rank, candidate.relative_path.clone())
}

fn poster_same_directory(media_path: &Path, poster_relative_path: &str) -> bool {
    let Some(media_dir) = media_path.parent() else {
        return false;
    };
    let Some(poster_parent) = poster_relative_path
        .rsplit_once('/')
        .map(|(parent, _)| parent)
    else {
        return false;
    };
    let media_dir_text = media_dir.to_string_lossy().replace('\\', "/");
    media_dir_text.ends_with(poster_parent)
}

fn poster_match_key(stem: &str) -> String {
    clip_base_stem(stem).to_ascii_lowercase()
}

fn clip_base_stem(stem: &str) -> String {
    let stem = stem.trim();
    if stem.is_empty() {
        return String::new();
    }
    let bytes = stem.as_bytes();
    for i in (1..bytes.len()).rev() {
        if bytes[i].is_ascii_alphabetic() && bytes[i + 1..].iter().all(u8::is_ascii_digit) {
            return stem[..i].trim().to_owned();
        }
    }
    stem.to_owned()
}

fn relative_transport_path(root: &Path, path: &Path) -> Result<String, ScanError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| ScanError::RelativePathEscapesRoot(path.to_path_buf()))?;
    let mut segments = Vec::new();

    for component in relative.components() {
        match component {
            Component::Normal(segment) => {
                let segment = segment
                    .to_str()
                    .ok_or_else(|| ScanError::NonUnicodePath(path.to_path_buf()))?;
                if segment.is_empty() || segment.contains('/') || segment.contains('\\') {
                    return Err(ScanError::InvalidRelativePath(path.to_path_buf()));
                }
                segments.push(segment.to_owned());
            }
            Component::CurDir => {}
            _ => return Err(ScanError::RelativePathEscapesRoot(path.to_path_buf())),
        }
    }

    if segments.is_empty() {
        return Err(ScanError::InvalidRelativePath(path.to_path_buf()));
    }

    Ok(segments.join("/"))
}

fn clip_fingerprint(path: &Path, file_size: u64) -> Result<String, ScanError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    hasher.update(FINGERPRINT_VERSION.as_bytes());
    hasher.update(file_size.to_le_bytes());

    hash_chunk(&mut file, 0, file_size, &mut hasher)?;
    if file_size > FINGERPRINT_SAMPLE_BYTES {
        let suffix_offset = file_size.saturating_sub(FINGERPRINT_SAMPLE_BYTES);
        hash_chunk(&mut file, suffix_offset, file_size, &mut hasher)?;
    }

    Ok(format!(
        "{}:{}",
        FINGERPRINT_VERSION,
        hex_digest(hasher.finalize().as_slice())
    ))
}

fn hash_chunk(
    file: &mut File,
    offset: u64,
    file_size: u64,
    hasher: &mut Sha256,
) -> Result<(), ScanError> {
    let remaining = file_size.saturating_sub(offset);
    let read_len = remaining.min(FINGERPRINT_SAMPLE_BYTES);
    let mut buffer = vec![0; read_len as usize];

    file.seek(SeekFrom::Start(offset))?;
    let bytes_read = file.read(&mut buffer)?;
    buffer.truncate(bytes_read);

    hasher.update(offset.to_le_bytes());
    hasher.update((bytes_read as u64).to_le_bytes());
    hasher.update(&buffer);
    Ok(())
}

#[derive(Debug, Clone)]
struct ClipTimestampEvidence {
    clip_created_at: Option<String>,
    clip_created_at_source: String,
    timestamp_evidence_json: String,
}

#[derive(Debug, Serialize)]
struct TimestampEvidenceEnvelope {
    schema_version: u32,
    chosen_source: String,
    chosen_at: Option<String>,
    filesystem_created_at: Option<String>,
    filesystem_modified_at: Option<String>,
    note: &'static str,
}

fn timestamp_evidence(metadata: &fs::Metadata) -> Result<ClipTimestampEvidence, ScanError> {
    let filesystem_created_at = metadata.created().ok().and_then(system_time_to_utc_text);
    let filesystem_modified_at = metadata.modified().ok().and_then(system_time_to_utc_text);
    let (clip_created_at_source, clip_created_at) = if let Some(created) = &filesystem_created_at {
        ("filesystem_created".to_owned(), Some(created.clone()))
    } else if let Some(modified) = &filesystem_modified_at {
        ("filesystem_modified".to_owned(), Some(modified.clone()))
    } else {
        ("unknown".to_owned(), None)
    };

    let evidence = TimestampEvidenceEnvelope {
        schema_version: 1,
        chosen_source: clip_created_at_source.clone(),
        chosen_at: clip_created_at.clone(),
        filesystem_created_at,
        filesystem_modified_at,
        note:
            "filesystem timestamps are fallback evidence until MediaProbe writes embedded metadata",
    };

    Ok(ClipTimestampEvidence {
        clip_created_at,
        clip_created_at_source,
        timestamp_evidence_json: serde_json::to_string(&evidence)?,
    })
}

fn system_time_to_utc_text(time: SystemTime) -> Option<String> {
    let duration = time.duration_since(UNIX_EPOCH).ok()?;
    Some(format_unix_utc(duration.as_secs(), duration.subsec_nanos()))
}

fn format_unix_utc(seconds: u64, nanos: u32) -> String {
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    let (year, month, day) = civil_from_unix_days(days);

    if nanos == 0 {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    } else {
        let mut fraction = format!("{nanos:09}");
        while fraction.ends_with('0') {
            fraction.pop();
        }
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{fraction}Z")
    }
}

fn civil_from_unix_days(days_since_unix_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);

    (year, month as u32, day as u32)
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug)]
pub enum ScanError {
    EmptySourceIdentity,
    UnknownSourceIdentity(String),
    SourceContentNotInitialized(String),
    RootIsNotDirectory { root: PathBuf },
    NonUnicodePath(PathBuf),
    InvalidRelativePath(PathBuf),
    RelativePathEscapesRoot(PathBuf),
    Io(io::Error),
    Database(rusqlite::Error),
    EvidenceJson(serde_json::Error),
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySourceIdentity => write!(f, "source identity is empty"),
            Self::UnknownSourceIdentity(source_identity) => {
                write!(f, "source identity is not recorded: {source_identity}")
            }
            Self::SourceContentNotInitialized(source_identity) => {
                write!(
                    f,
                    "content database is not initialized for source identity: {source_identity}"
                )
            }
            Self::RootIsNotDirectory { root } => {
                write!(
                    f,
                    "operator access root is not a directory: {}",
                    root.display()
                )
            }
            Self::NonUnicodePath(path) => {
                write!(f, "path is not valid Unicode: {}", path.display())
            }
            Self::InvalidRelativePath(path) => {
                write!(
                    f,
                    "relative path cannot be converted to QNC transport path: {}",
                    path.display()
                )
            }
            Self::RelativePathEscapesRoot(path) => {
                write!(
                    f,
                    "path is outside the source access root: {}",
                    path.display()
                )
            }
            Self::Io(error) => write!(f, "scanner I/O failed: {error}"),
            Self::Database(error) => write!(f, "scanner database write failed: {error}"),
            Self::EvidenceJson(error) => write!(f, "scanner evidence JSON failed: {error}"),
        }
    }
}

impl Error for ScanError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::EvidenceJson(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ScanError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for ScanError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

impl From<serde_json::Error> for ScanError {
    fn from(value: serde_json::Error) -> Self {
        Self::EvidenceJson(value)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Duration};

    use super::*;
    use crate::db::{upsert_content_source_identity, upsert_source_identity, SourceIdentityRecord};

    const SEEN_AT: &str = "2026-09-03T10:00:00Z";
    const SEEN_AGAIN_AT: &str = "2026-09-03T11:00:00Z";
    const SOURCE_IDENTITY: &str = "card:sn-001";

    #[test]
    fn scan_without_recorded_source_identity_fails_before_discovery() {
        let registry = db::open_registry_in_memory().unwrap();
        let content_db = db::open_content_in_memory().unwrap();
        let root = TempRoot::new();
        write_file(root.path().join("DCIM").join("A001_C001.mov"), b"clip-a");

        let err = scan_source(&registry, &content_db, request(root.path(), SEEN_AT))
            .expect_err("scanner must require source identity first");

        assert!(matches!(err, ScanError::UnknownSourceIdentity(_)));
        assert_eq!(clip_count(&content_db), 0);
    }

    #[test]
    fn scan_without_initialized_content_database_fails_before_discovery() {
        let registry = db::open_registry_in_memory().unwrap();
        let content_db = db::open_content_in_memory().unwrap();
        upsert_source_identity(&registry, &source_record(SEEN_AT)).unwrap();
        let root = TempRoot::new();
        write_file(root.path().join("DCIM").join("A001_C001.mov"), b"clip-a");

        let err = scan_source(&registry, &content_db, request(root.path(), SEEN_AT))
            .expect_err("scanner must require matching content DB source row first");

        assert!(matches!(err, ScanError::SourceContentNotInitialized(_)));
        assert_eq!(clip_count(&content_db), 0);
    }

    #[test]
    fn scanner_discovers_media_after_source_identity_is_recorded() {
        let (registry, content_db) = initialized_databases();
        let root = TempRoot::new();
        write_file(
            root.path()
                .join("DCIM")
                .join("100QNC")
                .join("A001_C001.MOV"),
            b"clip-a",
        );
        write_file(root.path().join("notes.txt"), b"not media");

        let report = scan_source(&registry, &content_db, request(root.path(), SEEN_AT)).unwrap();

        assert_eq!(report.files_seen, 2);
        assert_eq!(report.media_files_seen, 1);
        assert_eq!(report.clips_inserted, 1);
        assert_eq!(report.clips_updated, 0);
        assert_eq!(report.skipped_non_media, 1);

        let row: (String, String, String, Option<String>) = content_db
            .query_row(
                "
                SELECT relative_path, original_name, source_identity, extension
                FROM ingestqnc_clips
                ",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();

        assert_eq!(row.0, "DCIM/100QNC/A001_C001.MOV");
        assert_eq!(row.1, "A001_C001.MOV");
        assert_eq!(row.2, SOURCE_IDENTITY);
        assert_eq!(row.3.as_deref(), Some("mov"));
    }

    #[test]
    fn rediscovery_updates_existing_clip_without_duplicate() {
        let (registry, content_db) = initialized_databases();
        let root = TempRoot::new();
        write_file(root.path().join("DCIM").join("A001_C001.mov"), b"clip-a");

        let first = scan_source(&registry, &content_db, request(root.path(), SEEN_AT)).unwrap();
        let second =
            scan_source(&registry, &content_db, request(root.path(), SEEN_AGAIN_AT)).unwrap();

        assert_eq!(first.clips_inserted, 1);
        assert_eq!(second.clips_inserted, 0);
        assert_eq!(second.clips_updated, 1);
        assert_eq!(clip_count(&content_db), 1);

        let last_seen: String = content_db
            .query_row("SELECT last_seen_at FROM ingestqnc_clips", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(last_seen, SEEN_AGAIN_AT);
    }

    #[test]
    fn relative_path_is_os_neutral_and_does_not_include_access_root() {
        let (registry, content_db) = initialized_databases();
        let root = TempRoot::new();
        write_file(
            root.path()
                .join("PRIVATE")
                .join("CLIP")
                .join("A001 C001.mxf"),
            b"clip-a",
        );

        scan_source(&registry, &content_db, request(root.path(), SEEN_AT)).unwrap();

        let relative_path: String = content_db
            .query_row("SELECT relative_path FROM ingestqnc_clips", [], |row| {
                row.get(0)
            })
            .unwrap();

        assert_eq!(relative_path, "PRIVATE/CLIP/A001 C001.mxf");
        assert!(!relative_path.contains('\\'));
        assert!(!relative_path.contains(&root.path().to_string_lossy().to_string()));
    }

    #[test]
    fn fingerprint_does_not_include_access_root() {
        let (registry, content_db) = initialized_databases();
        let first_root = TempRoot::new();
        let second_root = TempRoot::new();
        write_file(first_root.path().join("A001_C001.mov"), b"same clip bytes");
        write_file(second_root.path().join("A001_C001.mov"), b"same clip bytes");

        let first =
            scan_source(&registry, &content_db, request(first_root.path(), SEEN_AT)).unwrap();
        let second = scan_source(
            &registry,
            &content_db,
            request(second_root.path(), SEEN_AGAIN_AT),
        )
        .unwrap();

        assert_eq!(first.clips_inserted, 1);
        assert_eq!(second.clips_inserted, 0);
        assert_eq!(second.clips_updated, 1);
        assert_eq!(clip_count(&content_db), 1);
    }

    #[test]
    fn timestamp_evidence_uses_filesystem_fallback_only() {
        let (registry, content_db) = initialized_databases();
        let root = TempRoot::new();
        write_file(root.path().join("A001_C001.mov"), b"clip-a");

        scan_source(&registry, &content_db, request(root.path(), SEEN_AT)).unwrap();

        let row: (Option<String>, String, String) = content_db
            .query_row(
                "
                SELECT clip_created_at, clip_created_at_source, timestamp_evidence_json
                FROM ingestqnc_clips
                ",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        assert!(row.0.is_some());
        assert!(matches!(
            row.1.as_str(),
            "filesystem_created" | "filesystem_modified"
        ));
        assert!(row
            .2
            .contains("filesystem timestamps are fallback evidence"));
    }

    #[test]
    fn scanner_records_card_poster_as_relative_transport_path() {
        let (registry, content_db) = initialized_databases();
        let root = TempRoot::new();
        write_file(
            root.path()
                .join("DCIM")
                .join("100QNC")
                .join("A001_C001.MOV"),
            b"clip-a",
        );
        write_file(
            root.path().join("THMBNL").join("A001_C001.THM"),
            b"jpeg-ish poster",
        );

        scan_source(&registry, &content_db, request(root.path(), SEEN_AT)).unwrap();

        let row: (Option<String>, Option<String>) = content_db
            .query_row(
                "
                SELECT poster_relative_path, poster_source
                FROM ingestqnc_clips
                ",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(row.0.as_deref(), Some("THMBNL/A001_C001.THM"));
        assert_eq!(row.1.as_deref(), Some("card_thm"));
        assert!(!row.0.unwrap().contains('\\'));
    }

    #[test]
    fn unix_epoch_timestamp_formats_as_utc() {
        assert_eq!(
            system_time_to_utc_text(UNIX_EPOCH + Duration::from_secs(0)).as_deref(),
            Some("1970-01-01T00:00:00Z")
        );
        assert_eq!(
            system_time_to_utc_text(UNIX_EPOCH + Duration::from_secs(86_400)).as_deref(),
            Some("1970-01-02T00:00:00Z")
        );
    }

    fn request(root: &Path, seen_at: &str) -> ScanRequest {
        ScanRequest::new(SOURCE_IDENTITY, root, seen_at)
    }

    fn source_record(seen_at: &str) -> SourceIdentityRecord {
        SourceIdentityRecord {
            source_identity: SOURCE_IDENTITY.into(),
            source_kind: "local_card".into(),
            display_name: "MEDIA_CARD_A".into(),
            transport_uri: "qnc+local://localhost/card/sn-001".into(),
            identity_evidence_json: r#"{"serial":"SN-001"}"#.into(),
            fallback_fingerprint: Some("fallback-a".into()),
            seen_at: seen_at.into(),
        }
    }

    fn initialized_databases() -> (Connection, Connection) {
        let registry = db::open_registry_in_memory().unwrap();
        let content_db = db::open_content_in_memory().unwrap();
        let source = source_record(SEEN_AT);
        upsert_source_identity(&registry, &source).unwrap();
        upsert_content_source_identity(&content_db, &source).unwrap();
        (registry, content_db)
    }

    fn clip_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM ingestqnc_clips", [], |row| row.get(0))
            .unwrap()
    }

    fn write_file(path: PathBuf, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("ingestqnc-scanner-{nanos}"));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
