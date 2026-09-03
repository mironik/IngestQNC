use std::{
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(test)]
use rusqlite::Connection;

#[cfg(test)]
use crate::db;
use crate::db::SourceIdentityRecord;

const MARKER_FILES: [&str; 4] = [
    ".qnc-source-identity.json",
    "qnc-source-identity.json",
    ".qnc_card_identity.json",
    "QNC_SOURCE_IDENTITY.json",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIdentityRequest {
    pub location: String,
    pub seen_at: String,
    pub display_name_hint: Option<String>,
}

impl SourceIdentityRequest {
    pub fn new(location: impl Into<String>, seen_at: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            seen_at: seen_at.into(),
            display_name_hint: None,
        }
    }

    #[cfg(test)]
    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name_hint = Some(display_name.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceIdentityConfidence {
    Strong,
    Medium,
    Fallback,
}

impl SourceIdentityConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strong => "strong",
            Self::Medium => "medium",
            Self::Fallback => "fallback",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedSourceIdentity {
    pub record: SourceIdentityRecord,
    pub confidence: SourceIdentityConfidence,
    pub identity_basis: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSourceEvidence {
    pub marker_source_identity: Option<String>,
    pub marker_source_kind: Option<String>,
    pub marker_display_name: Option<String>,
    pub marker_transport_uri: Option<String>,
    pub marker_path: Option<String>,
    pub media_serial: Option<String>,
    pub volume_uuid: Option<String>,
    pub volume_label: Option<String>,
    pub canonical_path: Option<String>,
    pub root_exists: bool,
    pub top_level_entries: Vec<String>,
    pub camera_layout: Vec<String>,
    pub provider_notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct NetworkSourceEvidence {
    input_location: String,
    host: String,
    share_or_gateway_path: String,
    transport_uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct IdentityEvidenceEnvelope {
    schema_version: u32,
    input_location: String,
    source_kind: String,
    transport_uri: String,
    identity_basis: String,
    confidence: String,
    os: String,
    arch: String,
    local: Option<LocalSourceEvidence>,
    network: Option<NetworkSourceEvidence>,
}

pub trait SourceIdentityEvidenceProvider {
    fn local_evidence(&self, root: &Path) -> LocalSourceEvidence;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct StdSourceIdentityEvidenceProvider;

impl SourceIdentityEvidenceProvider for StdSourceIdentityEvidenceProvider {
    fn local_evidence(&self, root: &Path) -> LocalSourceEvidence {
        let mut evidence = LocalSourceEvidence {
            canonical_path: fs::canonicalize(root)
                .ok()
                .map(|path| path.to_string_lossy().to_string()),
            root_exists: root.exists(),
            ..Default::default()
        };

        read_marker(root, &mut evidence);
        enrich_platform_volume_identity(root, &mut evidence);
        read_top_level_entries(root, &mut evidence);
        evidence.camera_layout = detect_camera_layout(&evidence.top_level_entries);
        evidence
    }
}

pub fn detect_source_identity(
    request: SourceIdentityRequest,
) -> Result<DetectedSourceIdentity, SourceIdentityError> {
    let provider = StdSourceIdentityEvidenceProvider;
    detect_source_identity_with_provider(request, &provider)
}

pub fn detect_source_identity_with_provider(
    request: SourceIdentityRequest,
    provider: &dyn SourceIdentityEvidenceProvider,
) -> Result<DetectedSourceIdentity, SourceIdentityError> {
    let location = request.location.trim();
    if location.is_empty() {
        return Err(SourceIdentityError::EmptyLocation);
    }
    if location.starts_with("file:") {
        return Err(SourceIdentityError::InvalidLocation(
            "non-QNC URI scheme cannot be used as a stable transport path".into(),
        ));
    }

    if let Some(network) = parse_intranet_location(location)? {
        return network_identity(&request, "intranet_gateway", network);
    }

    if let Some(network) = parse_lan_location(location)? {
        return network_identity(&request, "lan_share", network);
    }

    if let Some(local_transport) = parse_local_transport_location(location)? {
        return local_transport_identity(&request, local_transport);
    }

    local_identity(&request, provider)
}

#[cfg(test)]
pub fn detect_and_record_source_identity(
    conn: &Connection,
    request: SourceIdentityRequest,
) -> Result<DetectedSourceIdentity, SourceIdentityStoreError> {
    let detected = detect_source_identity(request)?;
    db::upsert_source_identity(conn, &detected.record)?;
    Ok(detected)
}

#[cfg(test)]
pub fn detect_and_record_source_identity_with_provider(
    conn: &Connection,
    request: SourceIdentityRequest,
    provider: &dyn SourceIdentityEvidenceProvider,
) -> Result<DetectedSourceIdentity, SourceIdentityStoreError> {
    let detected = detect_source_identity_with_provider(request, provider)?;
    db::upsert_source_identity(conn, &detected.record)?;
    Ok(detected)
}

fn local_identity(
    request: &SourceIdentityRequest,
    provider: &dyn SourceIdentityEvidenceProvider,
) -> Result<DetectedSourceIdentity, SourceIdentityError> {
    let path = request.location.trim().to_string();
    let path_obj = PathBuf::from(&path);
    let evidence = provider.local_evidence(&path_obj);
    let source_kind = evidence.marker_source_kind.clone().unwrap_or_else(|| {
        if !evidence.camera_layout.is_empty() || evidence.media_serial.is_some() {
            "local_card".into()
        } else {
            "local_volume".into()
        }
    });
    let display_name = request
        .display_name_hint
        .clone()
        .or_else(|| evidence.marker_display_name.clone())
        .or_else(|| evidence.volume_label.clone())
        .unwrap_or_else(|| display_name_from_path(&path));

    let (
        source_identity,
        confidence,
        identity_basis,
        fallback_fingerprint,
        generated_transport_uri,
    ) = if let Some(identity) = clean_optional(evidence.marker_source_identity.as_deref()) {
        (
            identity.clone(),
            SourceIdentityConfidence::Strong,
            "marker_source_identity".to_string(),
            root_signature_fingerprint(&evidence),
            format!(
                "qnc+local://localhost/source/{}",
                normalize_id_part(&identity)
            ),
        )
    } else if let Some(serial) = clean_optional(evidence.media_serial.as_deref()) {
        (
            format!("card:{}", normalize_id_part(&serial)),
            SourceIdentityConfidence::Strong,
            "media_serial".to_string(),
            root_signature_fingerprint(&evidence),
            format!("qnc+local://localhost/card/{}", normalize_id_part(&serial)),
        )
    } else if let Some(uuid) = clean_optional(evidence.volume_uuid.as_deref()) {
        (
            format!("volume:{}", normalize_id_part(&uuid)),
            SourceIdentityConfidence::Medium,
            "volume_uuid".to_string(),
            root_signature_fingerprint(&evidence),
            format!("qnc+local://localhost/volume/{}", normalize_id_part(&uuid)),
        )
    } else {
        let Some(fingerprint) = root_signature_fingerprint(&evidence) else {
            return Err(SourceIdentityError::InsufficientLocalEvidence(
                "missing marker, serial, volume UUID and root signature".into(),
            ));
        };
        (
            format!("local:{}", fingerprint),
            SourceIdentityConfidence::Fallback,
            "root_signature".to_string(),
            Some(fingerprint.clone()),
            format!("qnc+local://localhost/fallback/{fingerprint}"),
        )
    };
    let transport_uri = clean_optional(evidence.marker_transport_uri.as_deref())
        .filter(|uri| is_qnc_transport_uri(uri))
        .unwrap_or(generated_transport_uri);

    let envelope = IdentityEvidenceEnvelope {
        schema_version: 1,
        input_location: request.location.clone(),
        source_kind: source_kind.clone(),
        transport_uri: transport_uri.clone(),
        identity_basis: identity_basis.clone(),
        confidence: confidence.as_str().into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        local: Some(evidence),
        network: None,
    };

    Ok(DetectedSourceIdentity {
        record: SourceIdentityRecord {
            source_identity,
            source_kind,
            display_name,
            transport_uri,
            identity_evidence_json: serde_json::to_string(&envelope)
                .map_err(|error| SourceIdentityError::EvidenceJson(error.to_string()))?,
            fallback_fingerprint,
            seen_at: request.seen_at.clone(),
        },
        confidence,
        identity_basis,
    })
}

fn network_identity(
    request: &SourceIdentityRequest,
    db_source_kind: &str,
    network: NetworkSourceEvidence,
) -> Result<DetectedSourceIdentity, SourceIdentityError> {
    let identity_prefix = if db_source_kind == "lan_share" {
        "lan"
    } else {
        "intranet"
    };
    let path_part = if network.share_or_gateway_path.trim().is_empty() {
        String::new()
    } else {
        format!(":{}", normalize_id_part(&network.share_or_gateway_path))
    };
    let source_identity = format!(
        "{identity_prefix}:{}{}",
        normalize_id_part(&network.host),
        path_part
    );
    let display_name = request.display_name_hint.clone().unwrap_or_else(|| {
        format!(
            "{} {}",
            network.host,
            network.share_or_gateway_path.trim_matches('/')
        )
        .trim()
        .to_string()
    });
    let envelope = IdentityEvidenceEnvelope {
        schema_version: 1,
        input_location: request.location.clone(),
        source_kind: db_source_kind.into(),
        transport_uri: network.transport_uri.clone(),
        identity_basis: "transport_endpoint".into(),
        confidence: SourceIdentityConfidence::Medium.as_str().into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        local: None,
        network: Some(network.clone()),
    };

    Ok(DetectedSourceIdentity {
        record: SourceIdentityRecord {
            source_identity,
            source_kind: db_source_kind.into(),
            display_name,
            transport_uri: network.transport_uri,
            identity_evidence_json: serde_json::to_string(&envelope)
                .map_err(|error| SourceIdentityError::EvidenceJson(error.to_string()))?,
            fallback_fingerprint: None,
            seen_at: request.seen_at.clone(),
        },
        confidence: SourceIdentityConfidence::Medium,
        identity_basis: "transport_endpoint".into(),
    })
}

fn local_transport_identity(
    request: &SourceIdentityRequest,
    local: NetworkSourceEvidence,
) -> Result<DetectedSourceIdentity, SourceIdentityError> {
    let endpoint = local.share_or_gateway_path.trim_matches('/');
    let (source_identity, source_kind, confidence, identity_basis) =
        if let Some(rest) = endpoint.strip_prefix("card/") {
            (
                format!("card:{}", normalize_id_part(rest)),
                "local_card".to_string(),
                SourceIdentityConfidence::Medium,
                "local_transport_card".to_string(),
            )
        } else if let Some(rest) = endpoint.strip_prefix("volume/") {
            (
                format!("volume:{}", normalize_id_part(rest)),
                "local_volume".to_string(),
                SourceIdentityConfidence::Medium,
                "local_transport_volume".to_string(),
            )
        } else if let Some(rest) = endpoint.strip_prefix("fallback/") {
            (
                format!("local:{}", normalize_id_part(rest)),
                "local_volume".to_string(),
                SourceIdentityConfidence::Fallback,
                "local_transport_fallback".to_string(),
            )
        } else if let Some(rest) = endpoint.strip_prefix("source/") {
            (
                normalize_id_part(rest),
                "local_volume".to_string(),
                SourceIdentityConfidence::Medium,
                "local_transport_source".to_string(),
            )
        } else {
            return Err(SourceIdentityError::InvalidLocation(format!(
                "qnc+local endpoint must start with card/, volume/, source/ or fallback/: {}",
                request.location
            )));
        };

    let display_name = request
        .display_name_hint
        .clone()
        .unwrap_or_else(|| endpoint.replace('/', " "));
    let envelope = IdentityEvidenceEnvelope {
        schema_version: 1,
        input_location: request.location.clone(),
        source_kind: source_kind.clone(),
        transport_uri: local.transport_uri.clone(),
        identity_basis: identity_basis.clone(),
        confidence: confidence.as_str().into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        local: None,
        network: Some(local.clone()),
    };

    Ok(DetectedSourceIdentity {
        record: SourceIdentityRecord {
            source_identity,
            source_kind,
            display_name,
            transport_uri: local.transport_uri,
            identity_evidence_json: serde_json::to_string(&envelope)
                .map_err(|error| SourceIdentityError::EvidenceJson(error.to_string()))?,
            fallback_fingerprint: None,
            seen_at: request.seen_at.clone(),
        },
        confidence,
        identity_basis,
    })
}

fn parse_local_transport_location(
    location: &str,
) -> Result<Option<NetworkSourceEvidence>, SourceIdentityError> {
    let Some(rest) = location.strip_prefix("qnc+local://") else {
        return Ok(None);
    };
    let (host, path) = split_host_path(rest)?;
    Ok(Some(NetworkSourceEvidence {
        input_location: location.into(),
        transport_uri: format!("qnc+local://{}{}", normalize_host(&host), path),
        host,
        share_or_gateway_path: path.trim_start_matches('/').into(),
    }))
}

fn parse_intranet_location(
    location: &str,
) -> Result<Option<NetworkSourceEvidence>, SourceIdentityError> {
    for scheme in ["qnc+intranet://", "qnc://"] {
        if let Some(rest) = location.strip_prefix(scheme) {
            let (host, path) = split_host_path(rest)?;
            return Ok(Some(NetworkSourceEvidence {
                input_location: location.into(),
                transport_uri: format!("qnc+intranet://{}{}", normalize_host(&host), path),
                host,
                share_or_gateway_path: path.trim_start_matches('/').into(),
            }));
        }
    }
    Ok(None)
}

fn parse_lan_location(
    location: &str,
) -> Result<Option<NetworkSourceEvidence>, SourceIdentityError> {
    if let Some(rest) = location.strip_prefix("qnc+lan://") {
        let (host, path) = split_host_path(rest)?;
        return Ok(Some(NetworkSourceEvidence {
            input_location: location.into(),
            transport_uri: format!("qnc+lan://{}{}", normalize_host(&host), path),
            host,
            share_or_gateway_path: path.trim_start_matches('/').into(),
        }));
    }

    if let Some(rest) = location.strip_prefix("\\\\") {
        return parse_unc(rest, location, '\\');
    }
    if let Some(rest) = location.strip_prefix("//") {
        return parse_unc(rest, location, '/');
    }

    Ok(None)
}

fn parse_unc(
    rest: &str,
    original: &str,
    separator: char,
) -> Result<Option<NetworkSourceEvidence>, SourceIdentityError> {
    let parts: Vec<&str> = rest
        .split(separator)
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 2 {
        return Err(SourceIdentityError::InvalidLocation(format!(
            "UNC location must include server and share: {original}"
        )));
    }

    let host = parts[0].to_string();
    let share_path = parts[1..].join("/");
    Ok(Some(NetworkSourceEvidence {
        input_location: original.into(),
        transport_uri: format!("qnc+lan://{}/{}", normalize_host(&host), share_path),
        host,
        share_or_gateway_path: share_path,
    }))
}

#[cfg(windows)]
fn enrich_platform_volume_identity(root: &Path, evidence: &mut LocalSourceEvidence) {
    use std::{ffi::OsStr, ptr};

    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;

    let Some(volume_root) = windows_volume_root(root) else {
        evidence
            .provider_notes
            .push("windows volume root unavailable".into());
        return;
    };

    let root_wide = wide_null(OsStr::new(&volume_root));
    let mut volume_name = [0u16; 260];
    let mut serial_number = 0u32;

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
        evidence.provider_notes.push(format!(
            "windows volume identity unavailable: {volume_root}"
        ));
        return;
    }

    if evidence.volume_uuid.is_none() && serial_number != 0 {
        evidence.volume_uuid = Some(format!("windows-volume-serial-{serial_number:08x}"));
    }
    if evidence.volume_label.is_none() {
        let label = utf16_z(&volume_name);
        if !label.is_empty() {
            evidence.volume_label = Some(label);
        }
    }
    evidence
        .provider_notes
        .push(format!("windows volume root: {volume_root}"));
}

#[cfg(not(windows))]
fn enrich_platform_volume_identity(_root: &Path, evidence: &mut LocalSourceEvidence) {
    evidence.provider_notes.push(format!(
        "{} volume identity provider not active in this build",
        std::env::consts::OS
    ));
}

#[cfg(windows)]
fn windows_volume_root(path: &Path) -> Option<String> {
    use std::path::{Component, Prefix};

    let mut components = path.components();
    match components.next()? {
        Component::Prefix(prefix_component) => match prefix_component.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                Some(format!("{}:\\", letter as char))
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => Some(format!(
                "\\\\{}\\{}\\",
                server.to_string_lossy(),
                share.to_string_lossy()
            )),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(windows)]
fn wide_null(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn utf16_z(value: &[u16]) -> String {
    let len = value.iter().position(|ch| *ch == 0).unwrap_or(value.len());
    String::from_utf16_lossy(&value[..len])
}

fn split_host_path(rest: &str) -> Result<(String, String), SourceIdentityError> {
    let trimmed = rest.trim();
    let Some((host, path)) = trimmed.split_once('/') else {
        let host = trimmed.trim();
        if host.is_empty() {
            return Err(SourceIdentityError::InvalidLocation(
                "transport URI host is empty".into(),
            ));
        }
        return Ok((host.into(), String::new()));
    };
    if host.trim().is_empty() {
        return Err(SourceIdentityError::InvalidLocation(
            "transport URI host is empty".into(),
        ));
    }
    Ok((host.trim().into(), format!("/{}", path.trim_matches('/'))))
}

fn read_marker(root: &Path, evidence: &mut LocalSourceEvidence) {
    for name in MARKER_FILES {
        let path = root.join(name);
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(marker) = serde_json::from_slice::<SourceIdentityMarker>(&bytes) else {
            evidence
                .provider_notes
                .push(format!("marker parse failed: {}", path.display()));
            continue;
        };
        evidence.marker_path = Some(path.to_string_lossy().to_string());
        evidence.marker_source_identity = marker.source_identity;
        evidence.marker_source_kind = marker.source_kind;
        evidence.marker_display_name = marker.display_name;
        evidence.marker_transport_uri = marker.transport_uri;
        evidence.media_serial = first_present([
            marker.media_serial,
            marker.card_serial,
            marker.device_serial,
            marker.serial,
        ]);
        evidence.volume_uuid = first_present([marker.volume_uuid, marker.filesystem_uuid]);
        evidence.volume_label = marker.volume_label;
        return;
    }
}

fn read_top_level_entries(root: &Path, evidence: &mut LocalSourceEvidence) {
    let Ok(entries) = fs::read_dir(root) else {
        if evidence.root_exists {
            evidence
                .provider_notes
                .push(format!("read_dir failed: {}", root.display()));
        }
        return;
    };

    let mut names = Vec::new();
    for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }
    names.sort_by_key(|name| name.to_ascii_lowercase());
    evidence.top_level_entries = names;
}

fn detect_camera_layout(entries: &[String]) -> Vec<String> {
    let mut layout = Vec::new();
    for marker in [
        "DCIM", "PRIVATE", "CONTENTS", "XDROOT", "AVCHD", "BPAV", "CLIP",
    ] {
        if entries
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(marker))
        {
            layout.push(marker.to_string());
        }
    }
    layout
}

fn root_signature_fingerprint(evidence: &LocalSourceEvidence) -> Option<String> {
    let has_material = evidence.volume_label.is_some()
        || !evidence.top_level_entries.is_empty()
        || !evidence.camera_layout.is_empty();
    if !has_material {
        return None;
    }

    let payload = serde_json::json!({
        "volume_label": evidence.volume_label,
        "top_level_entries": evidence.top_level_entries,
        "camera_layout": evidence.camera_layout,
    });
    Some(short_hash(&payload.to_string()))
}

fn display_name_from_path(path: &str) -> String {
    let normalized = normalize_path_text(path);
    let trimmed = normalized.trim_end_matches('/');
    trimmed
        .rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

fn normalize_path_text(path: &str) -> String {
    let replaced = path.trim().replace('\\', "/");
    let trimmed = replaced.trim_end_matches('/').to_string();
    if is_windows_drive_root_text(&replaced) && trimmed.len() == 2 {
        format!("{trimmed}/")
    } else {
        trimmed
    }
}

fn is_windows_drive_root_text(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 3 && bytes[1] == b':' && bytes[2] == b'/'
}

fn normalize_host(host: &str) -> String {
    host.trim().to_ascii_lowercase()
}

fn is_qnc_transport_uri(uri: &str) -> bool {
    let uri = uri.trim();
    uri.starts_with("qnc+local://")
        || uri.starts_with("qnc+lan://")
        || uri.starts_with("qnc+intranet://")
}

fn normalize_id_part(value: &str) -> String {
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

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn first_present<const N: usize>(values: [Option<String>; N]) -> Option<String> {
    values
        .into_iter()
        .find_map(|value| clean_optional(value.as_deref()))
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[derive(Debug, Clone, Deserialize)]
struct SourceIdentityMarker {
    source_identity: Option<String>,
    source_kind: Option<String>,
    display_name: Option<String>,
    transport_uri: Option<String>,
    media_serial: Option<String>,
    card_serial: Option<String>,
    device_serial: Option<String>,
    serial: Option<String>,
    volume_uuid: Option<String>,
    filesystem_uuid: Option<String>,
    volume_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceIdentityError {
    EmptyLocation,
    InvalidLocation(String),
    InsufficientLocalEvidence(String),
    EvidenceJson(String),
}

impl fmt::Display for SourceIdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyLocation => write!(f, "source location is empty"),
            Self::InvalidLocation(message) => write!(f, "invalid source location: {message}"),
            Self::InsufficientLocalEvidence(message) => {
                write!(f, "insufficient local source identity evidence: {message}")
            }
            Self::EvidenceJson(message) => {
                write!(f, "source identity evidence JSON failed: {message}")
            }
        }
    }
}

impl Error for SourceIdentityError {}

#[cfg(test)]
#[derive(Debug)]
pub enum SourceIdentityStoreError {
    Detect(SourceIdentityError),
    Database(rusqlite::Error),
}

#[cfg(test)]
impl fmt::Display for SourceIdentityStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Detect(error) => write!(f, "{error}"),
            Self::Database(error) => write!(f, "{error}"),
        }
    }
}

#[cfg(test)]
impl Error for SourceIdentityStoreError {}

#[cfg(test)]
impl From<SourceIdentityError> for SourceIdentityStoreError {
    fn from(value: SourceIdentityError) -> Self {
        Self::Detect(value)
    }
}

#[cfg(test)]
impl From<rusqlite::Error> for SourceIdentityStoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[derive(Clone)]
    struct FakeProvider(LocalSourceEvidence);

    impl SourceIdentityEvidenceProvider for FakeProvider {
        fn local_evidence(&self, _root: &Path) -> LocalSourceEvidence {
            self.0.clone()
        }
    }

    fn request(location: &str) -> SourceIdentityRequest {
        SourceIdentityRequest::new(location, "2026-09-03T10:00:00Z")
    }

    #[test]
    fn local_card_serial_is_primary_identity() {
        let provider = FakeProvider(LocalSourceEvidence {
            media_serial: Some("SN-001".into()),
            volume_uuid: Some("windows-volume-serial-de666c9f".into()),
            volume_label: Some("MEDIA_CARD_A".into()),
            top_level_entries: vec!["DCIM".into(), "PRIVATE".into()],
            camera_layout: vec!["DCIM".into(), "PRIVATE".into()],
            root_exists: true,
            ..Default::default()
        });

        let detected =
            detect_source_identity_with_provider(request("operator-root"), &provider).unwrap();

        assert_eq!(detected.record.source_identity, "card:sn-001");
        assert_eq!(detected.record.source_kind, "local_card");
        assert_eq!(detected.record.display_name, "MEDIA_CARD_A");
        assert_eq!(
            detected.record.transport_uri,
            "qnc+local://localhost/card/sn-001"
        );
        assert_eq!(detected.confidence, SourceIdentityConfidence::Strong);
        assert_eq!(detected.identity_basis, "media_serial");
        assert!(detected.record.fallback_fingerprint.is_some());

        let evidence: serde_json::Value =
            serde_json::from_str(&detected.record.identity_evidence_json).unwrap();
        assert_eq!(
            evidence
                .pointer("/local/media_serial")
                .and_then(|v| v.as_str()),
            Some("SN-001")
        );
        assert_eq!(
            evidence
                .pointer("/local/volume_uuid")
                .and_then(|v| v.as_str()),
            Some("windows-volume-serial-de666c9f")
        );
        assert_eq!(
            evidence
                .pointer("/local/volume_label")
                .and_then(|v| v.as_str()),
            Some("MEDIA_CARD_A")
        );
    }

    #[test]
    fn local_volume_uuid_is_medium_confidence_identity() {
        let provider = FakeProvider(LocalSourceEvidence {
            volume_uuid: Some("VOL-123".into()),
            volume_label: Some("Archive".into()),
            root_exists: true,
            ..Default::default()
        });

        let detected = detect_source_identity_with_provider(request("operator-root"), &provider)
            .expect("detect volume");

        assert_eq!(detected.record.source_identity, "volume:vol-123");
        assert_eq!(detected.record.source_kind, "local_volume");
        assert_eq!(
            detected.record.transport_uri,
            "qnc+local://localhost/volume/vol-123"
        );
        assert_eq!(detected.confidence, SourceIdentityConfidence::Medium);
        assert_eq!(detected.identity_basis, "volume_uuid");
    }

    #[test]
    fn marker_source_identity_overrides_generated_identity() {
        let provider = FakeProvider(LocalSourceEvidence {
            marker_source_identity: Some("card:sony:explicit-sn".into()),
            marker_source_kind: Some("local_card".into()),
            marker_display_name: Some("Sony Card".into()),
            media_serial: Some("SN-IGNORED".into()),
            root_exists: true,
            ..Default::default()
        });

        let detected = detect_source_identity_with_provider(request("operator-root"), &provider)
            .expect("detect marker");

        assert_eq!(detected.record.source_identity, "card:sony:explicit-sn");
        assert_eq!(detected.record.display_name, "Sony Card");
        assert_eq!(
            detected.record.transport_uri,
            "qnc+local://localhost/source/card_sony_explicit-sn"
        );
        assert_eq!(detected.identity_basis, "marker_source_identity");
    }

    #[test]
    fn fallback_root_signature_is_stable_across_mount_paths() {
        let evidence = LocalSourceEvidence {
            top_level_entries: vec!["DCIM".into(), "PRIVATE".into()],
            camera_layout: vec!["DCIM".into(), "PRIVATE".into()],
            root_exists: true,
            ..Default::default()
        };
        let provider = FakeProvider(evidence);

        let first =
            detect_source_identity_with_provider(request("operator-root-a"), &provider).unwrap();
        let second =
            detect_source_identity_with_provider(request("operator-root-b"), &provider).unwrap();

        assert_eq!(first.record.source_identity, second.record.source_identity);
        assert!(first.record.source_identity.starts_with("local:"));
        assert!(first
            .record
            .transport_uri
            .starts_with("qnc+local://localhost/fallback/"));
        assert_eq!(first.confidence, SourceIdentityConfidence::Fallback);
    }

    #[test]
    fn local_source_without_os_neutral_evidence_is_rejected() {
        let provider = FakeProvider(LocalSourceEvidence::default());
        let err = detect_source_identity_with_provider(request("operator-root"), &provider)
            .expect_err("missing evidence should fail");

        assert!(matches!(
            err,
            SourceIdentityError::InsufficientLocalEvidence(_)
        ));
    }

    #[test]
    fn local_serial_transport_does_not_include_input_path() {
        let provider = FakeProvider(LocalSourceEvidence {
            media_serial: Some("SN-001".into()),
            volume_label: Some("MEDIA_CARD_A".into()),
            root_exists: true,
            ..Default::default()
        });
        let detected =
            detect_source_identity_with_provider(request("operator-root"), &provider).unwrap();

        assert!(!detected.record.transport_uri.contains("operator-root"));
        assert_eq!(
            detected.record.transport_uri,
            "qnc+local://localhost/card/sn-001"
        );
    }

    #[test]
    fn local_card_identity_does_not_depend_on_volume_label() {
        let media_a = FakeProvider(LocalSourceEvidence {
            media_serial: Some("SN-001".into()),
            volume_label: Some("MEDIA_CARD_A".into()),
            root_exists: true,
            ..Default::default()
        });
        let media_b = FakeProvider(LocalSourceEvidence {
            media_serial: Some("SN-001".into()),
            volume_label: Some("RENAMED_CARD".into()),
            root_exists: true,
            ..Default::default()
        });

        let detected_a =
            detect_source_identity_with_provider(request("operator-root-a"), &media_a).unwrap();
        let detected_b =
            detect_source_identity_with_provider(request("operator-root-b"), &media_b).unwrap();

        assert_eq!(detected_a.record.source_identity, "card:sn-001");
        assert_eq!(
            detected_a.record.source_identity,
            detected_b.record.source_identity
        );
        assert_eq!(detected_a.record.display_name, "MEDIA_CARD_A");
        assert_eq!(detected_b.record.display_name, "RENAMED_CARD");
    }

    #[test]
    fn local_card_uri_and_os_path_share_source_identity() {
        let provider = FakeProvider(LocalSourceEvidence {
            media_serial: Some("SN-001".into()),
            volume_label: Some("MEDIA_CARD_A".into()),
            root_exists: true,
            ..Default::default()
        });

        let from_os_path =
            detect_source_identity_with_provider(request("operator-root"), &provider).unwrap();
        let from_transport =
            detect_source_identity(request("qnc+local://localhost/card/SN-001")).unwrap();

        assert_eq!(from_os_path.record.source_identity, "card:sn-001");
        assert_eq!(
            from_os_path.record.source_identity,
            from_transport.record.source_identity
        );
    }

    #[test]
    fn qnc_lan_uri_becomes_lan_transport_identity() {
        let detected = detect_source_identity(request("qnc+lan://nas-qnc/ingest/cards/A001"))
            .expect("detect lan uri");

        assert_eq!(detected.record.source_kind, "lan_share");
        assert_eq!(
            detected.record.source_identity,
            "lan:nas-qnc:ingest_cards_a001"
        );
        assert_eq!(
            detected.record.transport_uri,
            "qnc+lan://nas-qnc/ingest/cards/A001"
        );
    }

    #[test]
    fn lan_uri_path_is_normalized() {
        let detected = detect_source_identity(request("qnc+lan://NAS-QNC/Ingest/Cards/A001"))
            .expect("detect lan uri");

        assert_eq!(
            detected.record.source_identity,
            "lan:nas-qnc:ingest_cards_a001"
        );
        assert_eq!(
            detected.record.transport_uri,
            "qnc+lan://nas-qnc/Ingest/Cards/A001"
        );
    }

    #[test]
    fn intranet_uri_path_becomes_gateway_identity() {
        let detected =
            detect_source_identity(request("qnc://Ingest-Gateway/cards/2026-09-03")).unwrap();

        assert_eq!(detected.record.source_kind, "intranet_gateway");
        assert_eq!(
            detected.record.source_identity,
            "intranet:ingest-gateway:cards_2026-09-03"
        );
        assert_eq!(
            detected.record.transport_uri,
            "qnc+intranet://ingest-gateway/cards/2026-09-03"
        );
    }

    #[test]
    fn qnc_local_card_uri_is_os_neutral_identity_input() {
        let detected = detect_source_identity(
            request("qnc+local://localhost/card/SN-001").with_display_name("Operator Card"),
        )
        .expect("detect qnc local");

        assert_eq!(detected.record.source_identity, "card:sn-001");
        assert_eq!(detected.record.source_kind, "local_card");
        assert_eq!(detected.record.display_name, "Operator Card");
        assert_eq!(
            detected.record.transport_uri,
            "qnc+local://localhost/card/SN-001"
        );
        assert_eq!(detected.identity_basis, "local_transport_card");
    }

    #[test]
    fn qnc_local_uri_rejects_os_path_like_endpoint() {
        let err = detect_source_identity(request("qnc+local://localhost/drive-root"))
            .expect_err("non-contract local endpoint should fail");

        assert!(matches!(err, SourceIdentityError::InvalidLocation(_)));
    }

    #[cfg(windows)]
    #[test]
    fn windows_volume_root_uses_drive_root_for_nested_paths() {
        assert_eq!(
            windows_volume_root(Path::new("E:\\DCIM\\100QNC")),
            Some("E:\\".into())
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_volume_root_uses_unc_share_root_for_nested_paths() {
        assert_eq!(
            windows_volume_root(Path::new("\\\\nas-qnc\\cards\\A001\\DCIM")),
            Some("\\\\nas-qnc\\cards\\".into())
        );
    }

    #[test]
    fn empty_location_is_rejected() {
        let err = detect_source_identity(request("   ")).expect_err("empty should fail");
        assert_eq!(err, SourceIdentityError::EmptyLocation);
    }

    #[test]
    fn marker_file_can_be_read_from_real_source_root() {
        let root = unique_temp_root();
        fs::create_dir_all(root.join("DCIM")).unwrap();
        fs::write(
            root.join(".qnc-source-identity.json"),
            r#"{
                "source_identity": "card:test:marker-sn",
                "source_kind": "local_card",
                "display_name": "Marker Card",
                "media_serial": "MARKER-SN",
                "volume_label": "MARKER"
            }"#,
        )
        .unwrap();

        let detected = detect_source_identity(request(&root.to_string_lossy())).unwrap();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(detected.record.source_identity, "card:test:marker-sn");
        assert_eq!(detected.record.display_name, "Marker Card");
        assert_eq!(
            detected.record.transport_uri,
            "qnc+local://localhost/source/card_test_marker-sn"
        );
        assert!(detected
            .record
            .identity_evidence_json
            .contains(".qnc-source-identity.json"));
    }

    #[test]
    fn detect_and_record_source_identity_upserts_database_row() {
        let conn = db::open_registry_in_memory().unwrap();
        let provider = FakeProvider(LocalSourceEvidence {
            media_serial: Some("SN-001".into()),
            volume_label: Some("MEDIA_CARD_A".into()),
            root_exists: true,
            ..Default::default()
        });
        let req = request("operator-root");

        let detected =
            detect_and_record_source_identity_with_provider(&conn, req.clone(), &provider).unwrap();
        let detected_again =
            detect_and_record_source_identity_with_provider(&conn, req, &provider).unwrap();

        assert_eq!(detected.record.source_identity, "card:sn-001");
        assert_eq!(
            detected_again.record.source_identity,
            detected.record.source_identity
        );

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM ingestqnc_sources", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn detect_and_record_source_identity_uses_standard_provider() {
        let conn = db::open_registry_in_memory().unwrap();
        let root = unique_temp_root();
        fs::create_dir_all(root.join("DCIM")).unwrap();
        fs::write(
            root.join(".qnc-source-identity.json"),
            r#"{
                "source_identity": "card:test:standard-provider",
                "source_kind": "local_card",
                "display_name": "Standard Provider Card"
            }"#,
        )
        .unwrap();

        let detected =
            detect_and_record_source_identity(&conn, request(&root.to_string_lossy())).unwrap();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(
            detected.record.source_identity,
            "card:test:standard-provider"
        );
        assert_eq!(detected.record.display_name, "Standard Provider Card");
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ingestqnc-source-identity-{nanos}"))
    }
}
