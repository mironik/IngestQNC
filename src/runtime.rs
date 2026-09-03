use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    db::{
        self, SourceModuleDatabaseRecord, CONTENT_SCHEMA_VERSION, MODULE_CONTENT,
        REGISTRY_SCHEMA_VERSION,
    },
    media_probe::{self, ProbeRequest},
    model::{ClipCard, FsEntry, SourceKind},
    scanner::{self, ScanRequest},
    source_identity::{self, SourceIdentityRequest},
};

const DEFAULT_DB_URI_BASE: &str = "qnc+local://localhost/ingest-db";

#[derive(Debug, Clone)]
pub struct RuntimeController {
    locator: DatabaseLocator,
}

impl RuntimeController {
    pub fn from_environment() -> Self {
        Self {
            locator: DatabaseLocator::from_environment(),
        }
    }

    pub fn identify_selected_source(
        &self,
        location: &str,
    ) -> Result<IdentifiedSourceRuntime, RuntimeError> {
        self.identify_selected_source_at(location, &utc_now_text())
    }

    fn identify_selected_source_at(
        &self,
        location: &str,
        seen_at: &str,
    ) -> Result<IdentifiedSourceRuntime, RuntimeError> {
        let location = location.trim();
        if location.is_empty() {
            return Err(RuntimeError::EmptySourceLocation);
        }

        let registry_path = self.locator.registry_path();
        create_parent_dir(&registry_path)?;
        let registry = db::open_registry_database(&registry_path)?;

        let detected =
            source_identity::detect_source_identity(SourceIdentityRequest::new(location, seen_at))?;
        db::upsert_source_identity(&registry, &detected.record)?;

        let content_path = self
            .locator
            .module_database_path(&detected.record.source_identity, MODULE_CONTENT);
        create_parent_dir(&content_path)?;
        let content_db = db::open_content_database(&content_path)?;
        db::upsert_content_source_identity(&content_db, &detected.record)?;

        let content_database_uri = self
            .locator
            .module_database_uri(&detected.record.source_identity, MODULE_CONTENT);
        db::upsert_source_module_database(
            &registry,
            &SourceModuleDatabaseRecord {
                source_identity: detected.record.source_identity.clone(),
                module_name: MODULE_CONTENT.into(),
                database_uri: content_database_uri.clone(),
                module_schema_version: CONTENT_SCHEMA_VERSION,
                evidence_json: module_database_evidence_json(MODULE_CONTENT, &content_path),
                seen_at: seen_at.into(),
            },
        )?;

        let source_identity = detected.record.source_identity.clone();
        let identity_basis = detected.identity_basis.clone();

        Ok(IdentifiedSourceRuntime {
            operator_location: location.into(),
            source_identity: detected.record.source_identity,
            source_kind: detected.record.source_kind,
            display_name: detected.record.display_name,
            transport_uri: detected.record.transport_uri,
            identity_basis: identity_basis.clone(),
            identity_label: identity_label(&identity_basis).into(),
            identity_value: identity_value_from_evidence(
                &detected.record.identity_evidence_json,
                &identity_basis,
                &source_identity,
            ),
            confidence: detected.confidence.as_str().into(),
            seen_at: seen_at.into(),
            registry_database_uri: self.locator.registry_database_uri(),
            registry_schema_version: REGISTRY_SCHEMA_VERSION,
            content_database_uri,
            content_schema_version: CONTENT_SCHEMA_VERSION,
        })
    }

    pub fn scan_identified_source(
        &self,
        source_identity: &str,
        operator_location: &str,
    ) -> Result<ScannedSourceRuntime, RuntimeError> {
        self.scan_identified_source_at(source_identity, operator_location, &utc_now_text())
    }

    fn scan_identified_source_at(
        &self,
        source_identity: &str,
        operator_location: &str,
        seen_at: &str,
    ) -> Result<ScannedSourceRuntime, RuntimeError> {
        let source_identity = source_identity.trim();
        if source_identity.is_empty() {
            return Err(RuntimeError::SourceNotIdentified);
        }

        let access_root = operator_access_root(operator_location)?;
        let registry = db::open_registry_database(self.locator.registry_path())?;
        let content_db = db::open_content_database(
            self.locator
                .module_database_path(source_identity, MODULE_CONTENT),
        )?;
        let report = scanner::scan_source(
            &registry,
            &content_db,
            ScanRequest::new(source_identity, access_root, seen_at),
        )?;
        let access_root = operator_access_root(operator_location)?;
        let clips = load_clip_cards(&content_db, Some(&access_root))?;

        Ok(ScannedSourceRuntime {
            report: ScanRuntimeReport {
                source_identity: report.source_identity,
                files_seen: report.files_seen,
                media_files_seen: report.media_files_seen,
                clips_inserted: report.clips_inserted,
                clips_updated: report.clips_updated,
                skipped_non_media: report.skipped_non_media,
            },
            clips,
        })
    }

    pub fn probe_selected_clips(
        &self,
        source_identity: &str,
        operator_location: &str,
        selected_clip_ids: Vec<String>,
    ) -> Result<ProbedSourceRuntime, RuntimeError> {
        self.probe_selected_clips_at(
            source_identity,
            operator_location,
            selected_clip_ids,
            &utc_now_text(),
        )
    }

    fn probe_selected_clips_at(
        &self,
        source_identity: &str,
        operator_location: &str,
        selected_clip_ids: Vec<String>,
        probed_at: &str,
    ) -> Result<ProbedSourceRuntime, RuntimeError> {
        let source_identity = source_identity.trim();
        if source_identity.is_empty() {
            return Err(RuntimeError::SourceNotIdentified);
        }

        let access_root = operator_access_root(operator_location)?;
        let content_db = db::open_content_database(
            self.locator
                .module_database_path(source_identity, MODULE_CONTENT),
        )?;
        let report = media_probe::probe_source_clips(
            &content_db,
            ProbeRequest::new(
                source_identity,
                access_root.clone(),
                selected_clip_ids.clone(),
                probed_at,
            ),
        )?;
        let mut clips = load_clip_cards(&content_db, Some(&access_root))?;
        mark_selected_clips(&mut clips, &selected_clip_ids);

        Ok(ProbedSourceRuntime {
            report: ProbeRuntimeReport {
                source_identity: report.source_identity,
                requested_clips: report.requested_clips,
                candidate_clips: report.candidate_clips,
                probes_ok: report.probes_ok,
                probes_error: report.probes_error,
                probes_skipped: report.probes_skipped,
            },
            clips,
        })
    }

    pub fn list_source_entries(
        &self,
        kind: SourceKind,
        path: &str,
    ) -> Result<SourceEntrySnapshot, RuntimeError> {
        match kind {
            SourceKind::Local => list_local_entries(path),
            SourceKind::Lan | SourceKind::Intranet => Ok(SourceEntrySnapshot {
                roots: false,
                path: path.trim().to_owned(),
                parent: None,
                entries: Vec::new(),
                selected_root_label: None,
            }),
        }
    }

    pub fn load_source_clips(
        &self,
        source_identity: &str,
        operator_location: &str,
    ) -> Result<Vec<ClipCard>, RuntimeError> {
        let source_identity = source_identity.trim();
        if source_identity.is_empty() {
            return Err(RuntimeError::SourceNotIdentified);
        }
        let access_root = operator_access_root(operator_location)?;
        let content_db = db::open_content_database(
            self.locator
                .module_database_path(source_identity, MODULE_CONTENT),
        )?;
        Ok(load_clip_cards(&content_db, Some(&access_root))?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifiedSourceRuntime {
    pub operator_location: String,
    pub source_identity: String,
    pub source_kind: String,
    pub display_name: String,
    pub transport_uri: String,
    pub identity_basis: String,
    pub identity_label: String,
    pub identity_value: String,
    pub confidence: String,
    pub seen_at: String,
    pub registry_database_uri: String,
    pub registry_schema_version: i64,
    pub content_database_uri: String,
    pub content_schema_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRuntimeReport {
    pub source_identity: String,
    pub files_seen: usize,
    pub media_files_seen: usize,
    pub clips_inserted: usize,
    pub clips_updated: usize,
    pub skipped_non_media: usize,
}

#[derive(Debug, Clone)]
pub struct ScannedSourceRuntime {
    pub report: ScanRuntimeReport,
    pub clips: Vec<ClipCard>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRuntimeReport {
    pub source_identity: String,
    pub requested_clips: usize,
    pub candidate_clips: usize,
    pub probes_ok: usize,
    pub probes_error: usize,
    pub probes_skipped: usize,
}

#[derive(Debug, Clone)]
pub struct ProbedSourceRuntime {
    pub report: ProbeRuntimeReport,
    pub clips: Vec<ClipCard>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntrySnapshot {
    pub roots: bool,
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<FsEntry>,
    pub selected_root_label: Option<String>,
}

#[derive(Debug, Clone)]
struct DatabaseLocator {
    root: PathBuf,
    uri_base: String,
}

impl DatabaseLocator {
    fn from_environment() -> Self {
        let root = std::env::var_os("INGESTQNC_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(default_database_root);
        let uri_base = std::env::var("INGESTQNC_DB_URI_BASE")
            .ok()
            .filter(|value| is_qnc_database_uri_base(value))
            .unwrap_or_else(|| DEFAULT_DB_URI_BASE.into());

        Self { root, uri_base }
    }

    #[cfg(test)]
    fn new(root: impl Into<PathBuf>, uri_base: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            uri_base: uri_base.into(),
        }
    }

    fn registry_path(&self) -> PathBuf {
        self.root.join("registry.sqlite")
    }

    fn registry_database_uri(&self) -> String {
        format!("{}/registry.sqlite", self.uri_base.trim_end_matches('/'))
    }

    fn module_database_path(&self, source_identity: &str, module_name: &str) -> PathBuf {
        self.root
            .join("sources")
            .join(uri_segment(source_identity))
            .join(format!("{}.sqlite", uri_segment(module_name)))
    }

    fn module_database_uri(&self, source_identity: &str, module_name: &str) -> String {
        format!(
            "{}/sources/{}/{}.sqlite",
            self.uri_base.trim_end_matches('/'),
            uri_segment(source_identity),
            uri_segment(module_name)
        )
    }
}

#[derive(Debug)]
pub enum RuntimeError {
    EmptySourceLocation,
    SourceNotIdentified,
    UnresolvableTransport(String),
    Io(io::Error),
    Database(rusqlite::Error),
    Scanner(scanner::ScanError),
    MediaProbe(media_probe::ProbeError),
    SourceIdentity(source_identity::SourceIdentityError),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySourceLocation => write!(f, "lokacija izvora nije zadana"),
            Self::SourceNotIdentified => write!(f, "izvor nije identificiran"),
            Self::UnresolvableTransport(message) => write!(f, "{message}"),
            Self::Io(error) => write!(f, "runtime I/O greska: {error}"),
            Self::Database(error) => write!(f, "runtime database greska: {error}"),
            Self::Scanner(error) => write!(f, "{error}"),
            Self::MediaProbe(error) => write!(f, "{error}"),
            Self::SourceIdentity(error) => write!(f, "{error}"),
        }
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::Scanner(error) => Some(error),
            Self::MediaProbe(error) => Some(error),
            Self::SourceIdentity(error) => Some(error),
            Self::EmptySourceLocation
            | Self::SourceNotIdentified
            | Self::UnresolvableTransport(_) => None,
        }
    }
}

impl From<io::Error> for RuntimeError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for RuntimeError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

impl From<scanner::ScanError> for RuntimeError {
    fn from(value: scanner::ScanError) -> Self {
        Self::Scanner(value)
    }
}

impl From<media_probe::ProbeError> for RuntimeError {
    fn from(value: media_probe::ProbeError) -> Self {
        Self::MediaProbe(value)
    }
}

impl From<source_identity::SourceIdentityError> for RuntimeError {
    fn from(value: source_identity::SourceIdentityError) -> Self {
        Self::SourceIdentity(value)
    }
}

fn default_database_root() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("ingestqnc-data")
}

fn list_local_entries(path: &str) -> Result<SourceEntrySnapshot, RuntimeError> {
    let path = path.trim();
    if path.is_empty() {
        return Ok(SourceEntrySnapshot {
            roots: true,
            path: String::new(),
            parent: None,
            entries: local_roots(),
            selected_root_label: None,
        });
    }

    let root = PathBuf::from(path);
    let metadata = fs::metadata(&root)?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("nije direktorij: {}", root.display()),
        )
        .into());
    }

    let mut entries = fs::read_dir(&root)?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_str()?.to_owned();
            Some(FsEntry {
                name,
                path: path.to_string_lossy().to_string(),
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.name.to_ascii_lowercase());

    Ok(SourceEntrySnapshot {
        roots: false,
        path: root.to_string_lossy().to_string(),
        parent: local_parent_path(&root),
        entries,
        selected_root_label: selected_local_root_label(&root),
    })
}

#[cfg(windows)]
fn local_roots() -> Vec<FsEntry> {
    (b'A'..=b'Z')
        .filter_map(|letter| {
            let drive = format!("{}:\\", letter as char);
            Path::new(&drive).is_dir().then(|| FsEntry {
                name: local_root_label(&drive),
                path: drive,
            })
        })
        .collect()
}

#[cfg(windows)]
fn local_root_label(drive: &str) -> String {
    let display_drive = drive.trim_end_matches('\\');
    let Some(info) = windows_volume_info(drive) else {
        return display_drive.to_owned();
    };

    let mut parts = vec![display_drive.to_owned()];
    if let Some(serial) = info.serial {
        parts.push(serial);
    }
    if let Some(name) = info.name.filter(|name| !name.trim().is_empty()) {
        parts.push(name);
    }
    parts.join("   ")
}

#[cfg(windows)]
fn selected_local_root_label(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();
    if bytes.len() < 2 || bytes[1] != b':' {
        return None;
    }
    let drive = format!("{}:\\", text.chars().next()?.to_ascii_uppercase());
    Some(local_root_label(&drive))
}

#[cfg(windows)]
struct WindowsVolumeInfo {
    name: Option<String>,
    serial: Option<String>,
}

#[cfg(windows)]
fn windows_volume_info(root: &str) -> Option<WindowsVolumeInfo> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr};

    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;

    let mut volume_name = [0u16; 260];
    let mut serial_number = 0u32;
    let root_wide = OsStr::new(root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let ok = unsafe {
        GetVolumeInformationW(
            root_wide.as_ptr(),
            volume_name.as_mut_ptr(),
            volume_name.len() as u32,
            &mut serial_number,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        return None;
    }

    let name = String::from_utf16_lossy(
        &volume_name[..volume_name
            .iter()
            .position(|ch| *ch == 0)
            .unwrap_or(volume_name.len())],
    );

    Some(WindowsVolumeInfo {
        name: (!name.trim().is_empty()).then(|| name.trim().to_owned()),
        serial: (serial_number != 0).then(|| format!("{serial_number:08x}")),
    })
}

#[cfg(not(windows))]
fn local_roots() -> Vec<FsEntry> {
    let mut roots = vec![FsEntry {
        name: "/".into(),
        path: "/".into(),
    }];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        if home.is_dir() {
            roots.push(FsEntry {
                name: "home".into(),
                path: home.to_string_lossy().to_string(),
            });
        }
    }
    roots
}

#[cfg(not(windows))]
fn selected_local_root_label(path: &Path) -> Option<String> {
    if path.starts_with("/") {
        Some("/".into())
    } else {
        None
    }
}

fn local_parent_path(path: &Path) -> Option<String> {
    path.parent()
        .map(|parent| parent.to_string_lossy().to_string())
        .filter(|parent| !parent.is_empty())
        .or_else(|| Some(String::new()))
}

fn operator_access_root(location: &str) -> Result<PathBuf, RuntimeError> {
    let location = location.trim();
    if location.is_empty() {
        return Err(RuntimeError::EmptySourceLocation);
    }
    if location.starts_with("file:") {
        return Err(RuntimeError::UnresolvableTransport(
            "file: URI nije QNC transport i ne smije biti stabilni ingest ulaz".into(),
        ));
    }
    if let Some(rest) = location.strip_prefix("qnc+lan://") {
        return qnc_lan_access_root(rest);
    }
    if let Some(rest) = location.strip_prefix("qnc+intranet://") {
        return qnc_mapped_access_root("INGESTQNC_INTRANET_MOUNT_ROOT", rest, "intranet");
    }
    if let Some(rest) = location.strip_prefix("qnc://") {
        return qnc_mapped_access_root("INGESTQNC_INTRANET_MOUNT_ROOT", rest, "intranet");
    }
    if location.starts_with("qnc+local://") {
        return Err(RuntimeError::UnresolvableTransport(
            "qnc+local URI je stabilni identitet; za scan treba trenutni lokalni pristupni direktorij"
                .into(),
        ));
    }

    Ok(PathBuf::from(location))
}

fn qnc_lan_access_root(rest: &str) -> Result<PathBuf, RuntimeError> {
    if let Some(path) = mapped_qnc_access_root("INGESTQNC_LAN_MOUNT_ROOT", rest)? {
        return Ok(path);
    }

    #[cfg(windows)]
    {
        let (host, path) = split_transport_endpoint(rest)?;
        let mut unc = String::from("\\\\");
        unc.push_str(&host);
        if !path.is_empty() {
            unc.push('\\');
            unc.push_str(&path.replace('/', "\\"));
        }
        return Ok(PathBuf::from(unc));
    }

    #[cfg(not(windows))]
    {
        Err(RuntimeError::UnresolvableTransport(
            "qnc+lan URI treba lokalni mount root kroz INGESTQNC_LAN_MOUNT_ROOT".into(),
        ))
    }
}

fn qnc_mapped_access_root(env_key: &str, rest: &str, label: &str) -> Result<PathBuf, RuntimeError> {
    mapped_qnc_access_root(env_key, rest)?.ok_or_else(|| {
        RuntimeError::UnresolvableTransport(format!(
            "qnc+{label} URI treba lokalni mount root kroz {env_key}"
        ))
    })
}

fn mapped_qnc_access_root(env_key: &str, rest: &str) -> Result<Option<PathBuf>, RuntimeError> {
    let Some(mount_root) = std::env::var_os(env_key).map(PathBuf::from) else {
        return Ok(None);
    };
    let (host, path) = split_transport_endpoint(rest)?;
    let mut root = mount_root.join(uri_segment(&host));
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        root = root.join(segment);
    }
    Ok(Some(root))
}

fn split_transport_endpoint(rest: &str) -> Result<(String, String), RuntimeError> {
    let trimmed = rest.trim();
    let Some((host, path)) = trimmed.split_once('/') else {
        if trimmed.is_empty() {
            return Err(RuntimeError::UnresolvableTransport(
                "QNC transport host nije zadan".into(),
            ));
        }
        return Ok((trimmed.into(), String::new()));
    };
    if host.trim().is_empty() {
        return Err(RuntimeError::UnresolvableTransport(
            "QNC transport host nije zadan".into(),
        ));
    }
    Ok((host.trim().into(), path.trim_matches('/').into()))
}

fn load_clip_cards(
    content_db: &rusqlite::Connection,
    access_root: Option<&Path>,
) -> rusqlite::Result<Vec<ClipCard>> {
    let mut stmt = content_db.prepare(
        "
        SELECT
            clip_id,
            original_name,
            COALESCE(duration_sec, 0.0),
            source_identity,
            clip_fingerprint,
            COALESCE(clip_created_at, ''),
            latest_probe_id,
            latest_probe_status,
            poster_relative_path,
            poster_source
        FROM ingestqnc_catalog_read_v1
        ORDER BY relative_path COLLATE NOCASE
        ",
    )?;
    let rows = stmt.query_map([], |row| {
        let latest_probe_id: Option<String> = row.get(6)?;
        let latest_probe_status: Option<String> = row.get(7)?;
        let poster_relative_path: Option<String> = row.get(8)?;
        Ok(ClipCard {
            id: row.get(0)?,
            name: row.get(1)?,
            duration_sec: row.get(2)?,
            ingest_status: clip_status(latest_probe_id.as_deref(), latest_probe_status.as_deref()),
            selected: false,
            source_identity: row.get(3)?,
            clip_fingerprint: row.get(4)?,
            clip_created_at: row.get(5)?,
            poster_access_path: poster_relative_path.as_deref().and_then(|relative| {
                access_root.and_then(|root| runtime_access_path(root, relative))
            }),
            poster_relative_path,
            poster_source: row.get(9)?,
        })
    })?;
    rows.collect()
}

fn clip_status(probe_id: Option<&str>, probe_status: Option<&str>) -> String {
    match probe_status.map(|status| status.trim().to_ascii_lowercase()) {
        Some(status) if status == "error" => "error".into(),
        Some(status) if status == "ok" => "done".into(),
        _ if probe_id.is_some() => "done".into(),
        _ => "scanned".into(),
    }
}

fn runtime_access_path(root: &Path, relative_path: &str) -> Option<String> {
    let mut out = root.to_path_buf();
    for segment in relative_path.split('/') {
        let segment = segment.trim();
        if segment.is_empty() || segment.contains('\\') {
            return None;
        }
        let path = Path::new(segment);
        if path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return None;
        }
        out.push(segment);
    }
    Some(out.to_string_lossy().to_string())
}

fn identity_label(identity_basis: &str) -> &'static str {
    match identity_basis {
        "marker_source_identity" => "Marker",
        "media_serial" => "Serial",
        "volume_uuid" => "Volume",
        "root_signature" => "Fingerprint",
        "transport_endpoint" => "Endpoint",
        "local_transport_card" => "Card",
        "local_transport_volume" => "Volume",
        "local_transport_source" => "Source",
        "local_transport_fallback" => "Fallback",
        _ => "Identity",
    }
}

fn identity_value_from_evidence(
    evidence_json: &str,
    identity_basis: &str,
    fallback: &str,
) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(evidence_json) else {
        return fallback.to_owned();
    };
    match identity_basis {
        "media_serial" => value
            .pointer("/local/media_serial")
            .and_then(serde_json::Value::as_str),
        "volume_uuid" => value
            .pointer("/local/volume_uuid")
            .and_then(serde_json::Value::as_str),
        "transport_endpoint" => value
            .pointer("/network/share_or_gateway_path")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                value
                    .pointer("/network/host")
                    .and_then(serde_json::Value::as_str)
            }),
        "marker_source_identity" => Some(fallback),
        "root_signature" => value
            .pointer("/local/canonical_path")
            .and_then(serde_json::Value::as_str),
        _ => Some(fallback),
    }
    .unwrap_or(fallback)
    .to_owned()
}

fn mark_selected_clips(clips: &mut [ClipCard], selected_clip_ids: &[String]) {
    if selected_clip_ids.is_empty() {
        return;
    }
    let selected = selected_clip_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .collect::<std::collections::HashSet<_>>();
    for clip in clips {
        clip.selected = selected.contains(clip.id.as_str());
    }
}

fn create_parent_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn module_database_evidence_json(module_name: &str, path: &Path) -> String {
    serde_json::json!({
        "schema_version": 1,
        "module_name": module_name,
        "local_path_evidence": path.to_string_lossy(),
        "note": "raw local path is runtime evidence only; persisted database_uri is QNC transport"
    })
    .to_string()
}

fn is_qnc_database_uri_base(value: &str) -> bool {
    let value = value.trim();
    value.starts_with("qnc+local://")
        || value.starts_with("qnc+lan://")
        || value.starts_with("qnc+intranet://")
}

fn uri_segment(value: &str) -> String {
    let mut out = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out.trim_matches('_').to_string()
}

fn utc_now_text() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_unix_utc(duration.as_secs())
}

fn format_unix_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    let (year, month, day) = civil_from_unix_days(days);

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
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

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::{params, Connection};

    use super::*;
    use crate::db::MODULE_CONTENT;

    #[test]
    fn identify_selected_source_creates_registry_and_content_database() {
        let db_root = unique_temp_root("db");
        let source_root = unique_temp_root("source");
        fs::create_dir_all(source_root.join("DCIM")).unwrap();
        fs::write(
            source_root.join(".qnc-source-identity.json"),
            r#"{
                "source_identity": "card:test:sn-001",
                "source_kind": "local_card",
                "display_name": "Test Card",
                "media_serial": "SN-001"
            }"#,
        )
        .unwrap();
        let controller = RuntimeController {
            locator: DatabaseLocator::new(&db_root, "qnc+local://localhost/ingest-db"),
        };

        let identified = controller
            .identify_selected_source_at(&source_root.to_string_lossy(), "2026-09-03T10:00:00Z")
            .unwrap();

        assert_eq!(identified.source_identity, "card:test:sn-001");
        assert_eq!(identified.display_name, "Test Card");
        assert_eq!(identified.content_schema_version, CONTENT_SCHEMA_VERSION);
        assert!(identified
            .content_database_uri
            .starts_with("qnc+local://localhost/ingest-db/sources/card_test_sn-001/"));
        assert!(!identified
            .content_database_uri
            .contains(&db_root.to_string_lossy().to_string()));

        let registry = Connection::open(db_root.join("registry.sqlite")).unwrap();
        let registry_count: i64 = registry
            .query_row("SELECT COUNT(*) FROM ingestqnc_sources", [], |row| {
                row.get(0)
            })
            .unwrap();
        let module_uri: String = registry
            .query_row(
                "
                SELECT database_uri
                FROM ingestqnc_source_module_databases
                WHERE source_identity = ?1 AND module_name = ?2
                ",
                params!["card:test:sn-001", MODULE_CONTENT],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(registry_count, 1);
        assert_eq!(module_uri, identified.content_database_uri);

        let content = Connection::open(
            db_root
                .join("sources")
                .join("card_test_sn-001")
                .join("content.sqlite"),
        )
        .unwrap();
        let content_count: i64 = content
            .query_row(
                "SELECT COUNT(*) FROM ingestqnc_content_source_meta",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(content_count, 1);

        fs::remove_dir_all(db_root).ok();
        fs::remove_dir_all(source_root).ok();
    }

    #[test]
    fn scan_identified_source_writes_content_database_and_returns_clip_cards() {
        let db_root = unique_temp_root("db");
        let source_root = unique_temp_root("source");
        fs::create_dir_all(source_root.join("DCIM")).unwrap();
        fs::write(
            source_root.join(".qnc-source-identity.json"),
            r#"{
                "source_identity": "card:test:scan-sn-001",
                "source_kind": "local_card",
                "display_name": "Scan Card",
                "media_serial": "SCAN-SN-001"
            }"#,
        )
        .unwrap();
        fs::write(source_root.join("DCIM").join("A001_C001.MOV"), b"clip-a").unwrap();
        fs::write(source_root.join("notes.txt"), b"not media").unwrap();
        let controller = RuntimeController {
            locator: DatabaseLocator::new(&db_root, "qnc+local://localhost/ingest-db"),
        };
        let identified = controller
            .identify_selected_source_at(&source_root.to_string_lossy(), "2026-09-03T10:00:00Z")
            .unwrap();

        let scanned = controller
            .scan_identified_source_at(
                &identified.source_identity,
                &identified.operator_location,
                "2026-09-03T10:01:00Z",
            )
            .unwrap();

        assert_eq!(scanned.report.files_seen, 3);
        assert_eq!(scanned.report.media_files_seen, 1);
        assert_eq!(scanned.report.clips_inserted, 1);
        assert_eq!(scanned.report.clips_updated, 0);
        assert_eq!(scanned.report.skipped_non_media, 2);
        assert_eq!(scanned.clips.len(), 1);
        assert_eq!(scanned.clips[0].name, "A001_C001.MOV");
        assert_eq!(scanned.clips[0].source_identity, "card:test:scan-sn-001");

        fs::remove_dir_all(db_root).ok();
        fs::remove_dir_all(source_root).ok();
    }

    #[test]
    fn locator_accepts_lan_and_intranet_database_uri_bases() {
        let lan = DatabaseLocator::new("runtime-root", "qnc+lan://storage/ingest-db");
        let intranet = DatabaseLocator::new("runtime-root", "qnc+intranet://gateway/ingest-db");

        assert_eq!(
            lan.module_database_uri("card:test:sn-001", MODULE_CONTENT),
            "qnc+lan://storage/ingest-db/sources/card_test_sn-001/content.sqlite"
        );
        assert_eq!(
            intranet.registry_database_uri(),
            "qnc+intranet://gateway/ingest-db/registry.sqlite"
        );
    }

    #[test]
    fn list_source_entries_lists_local_children_without_persisting_them() {
        let root = unique_temp_root("local-root");
        fs::create_dir_all(root.join("DCIM")).unwrap();
        fs::create_dir_all(root.join("PRIVATE")).unwrap();
        fs::write(root.join("clip.mov"), b"not listed by browser").unwrap();
        let controller = RuntimeController {
            locator: DatabaseLocator::new(
                unique_temp_root("db"),
                "qnc+local://localhost/ingest-db",
            ),
        };

        let snapshot = controller
            .list_source_entries(SourceKind::Local, &root.to_string_lossy())
            .unwrap();

        assert!(!snapshot.roots);
        assert_eq!(
            snapshot
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["DCIM", "PRIVATE"]
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn list_source_entries_keeps_lan_and_intranet_as_operator_input() {
        let controller = RuntimeController {
            locator: DatabaseLocator::new("runtime-root", "qnc+local://localhost/ingest-db"),
        };

        let lan = controller
            .list_source_entries(SourceKind::Lan, "qnc+lan://nas-qnc/cards/A001")
            .unwrap();
        let intranet = controller
            .list_source_entries(SourceKind::Intranet, "qnc+intranet://gateway/cards/A001")
            .unwrap();

        assert_eq!(lan.path, "qnc+lan://nas-qnc/cards/A001");
        assert_eq!(intranet.path, "qnc+intranet://gateway/cards/A001");
        assert!(lan.entries.is_empty());
        assert!(intranet.entries.is_empty());
    }

    #[test]
    fn qnc_lan_and_intranet_use_transport_mapping_for_runtime_access() {
        assert_eq!(
            access_root_with_mapping("C:/mnt/lan", "nas-qnc/cards/A001",),
            PathBuf::from("C:/mnt/lan")
                .join("nas-qnc")
                .join("cards")
                .join("A001")
        );
        assert_eq!(
            access_root_with_mapping("C:/mnt/intranet", "gateway/cards/A001"),
            PathBuf::from("C:/mnt/intranet")
                .join("gateway")
                .join("cards")
                .join("A001")
        );
    }

    #[test]
    fn empty_source_location_is_rejected_before_database_creation() {
        let db_root = unique_temp_root("db");
        let controller = RuntimeController {
            locator: DatabaseLocator::new(&db_root, "qnc+local://localhost/ingest-db"),
        };

        let err = controller
            .identify_selected_source_at(" ", "2026-09-03T10:00:00Z")
            .expect_err("empty location should fail");

        assert!(matches!(err, RuntimeError::EmptySourceLocation));
        assert!(!db_root.exists());
    }

    fn access_root_with_mapping(mount_root: &str, rest: &str) -> PathBuf {
        let (host, path) = split_transport_endpoint(rest).unwrap();
        let mut root = PathBuf::from(mount_root).join(uri_segment(&host));
        for segment in path.split('/').filter(|segment| !segment.is_empty()) {
            root = root.join(segment);
        }
        root
    }

    fn unique_temp_root(kind: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ingestqnc-runtime-{kind}-{nanos}"))
    }
}
