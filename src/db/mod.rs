use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

pub const REGISTRY_SCHEMA_VERSION: i64 = 1;
pub const CONTENT_SCHEMA_VERSION: i64 = 2;
#[cfg(test)]
pub const FILMSTRIP_SCHEMA_VERSION: i64 = 1;
#[cfg(test)]
pub const WAVE_SCHEMA_VERSION: i64 = 1;

pub const MODULE_CONTENT: &str = "content";
#[cfg(test)]
pub const MODULE_FILMSTRIP: &str = "filmstrip";
#[cfg(test)]
pub const MODULE_WAVE: &str = "wave";

const REGISTRY_APPLICATION_ID: i64 = 0x514E_4352;
const CONTENT_APPLICATION_ID: i64 = 0x514E_4343;
#[cfg(test)]
const FILMSTRIP_APPLICATION_ID: i64 = 0x514E_4346;
#[cfg(test)]
const WAVE_APPLICATION_ID: i64 = 0x514E_4357;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIdentityRecord {
    pub source_identity: String,
    pub source_kind: String,
    pub display_name: String,
    pub transport_uri: String,
    pub identity_evidence_json: String,
    pub fallback_fingerprint: Option<String>,
    pub seen_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceModuleDatabaseRecord {
    pub source_identity: String,
    pub module_name: String,
    pub database_uri: String,
    pub module_schema_version: i64,
    pub evidence_json: String,
    pub seen_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipDiscoveryRecord {
    pub source_identity: String,
    pub clip_fingerprint: String,
    pub relative_path: String,
    pub original_name: String,
    pub extension: Option<String>,
    pub poster_relative_path: Option<String>,
    pub poster_source: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub clip_created_at: Option<String>,
    pub clip_created_at_source: String,
    pub clip_created_at_offset: Option<String>,
    pub timestamp_evidence_json: String,
    pub seen_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaProbeSnapshotRecord {
    pub probe_id: String,
    pub clip_id: String,
    pub probe_version: String,
    pub probed_at: String,
    pub status: String,
    pub duration_sec: Option<f64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub fps_num: Option<i64>,
    pub fps_den: Option<i64>,
    pub raw_probe_json: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipCreationTimestampUpdate {
    pub clip_id: String,
    pub clip_created_at: String,
    pub clip_created_at_source: String,
    pub clip_created_at_offset: Option<String>,
    pub timestamp_evidence_json: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertKind {
    Inserted,
    Updated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipUpsertResult {
    pub clip_id: String,
    pub kind: UpsertKind,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeSnapshotSummary {
    pub probe_id: String,
    pub clip_id: String,
    pub probe_version: String,
    pub status: String,
    pub duration_sec: Option<f64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

pub fn open_registry_database(path: impl AsRef<Path>) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    configure_connection(&conn)?;
    ensure_registry_schema(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_registry_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure_connection(&conn)?;
    ensure_registry_schema(&conn)?;
    Ok(conn)
}

pub fn open_content_database(path: impl AsRef<Path>) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    configure_connection(&conn)?;
    ensure_content_schema(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_content_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure_connection(&conn)?;
    ensure_content_schema(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_filmstrip_database(path: impl AsRef<Path>) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    configure_connection(&conn)?;
    ensure_filmstrip_schema(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_filmstrip_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure_connection(&conn)?;
    ensure_filmstrip_schema(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_wave_database(path: impl AsRef<Path>) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    configure_connection(&conn)?;
    ensure_wave_schema(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_wave_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure_connection(&conn)?;
    ensure_wave_schema(&conn)?;
    Ok(conn)
}

pub fn configure_connection(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        ",
    )
}

pub fn ensure_registry_schema(conn: &Connection) -> rusqlite::Result<()> {
    ensure_schema(
        conn,
        REGISTRY_SCHEMA_VERSION,
        "registry",
        migrate_registry_v1,
    )
}

pub fn ensure_content_schema(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > CONTENT_SCHEMA_VERSION {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "IngestQNC content DB version {version} is newer than supported version {CONTENT_SCHEMA_VERSION}"
        )));
    }
    if version < 1 {
        migrate_content_v1(conn)?;
        return Ok(());
    }
    if version < 2 {
        migrate_content_v2(conn)?;
    }
    Ok(())
}

#[cfg(test)]
pub fn ensure_filmstrip_schema(conn: &Connection) -> rusqlite::Result<()> {
    ensure_schema(
        conn,
        FILMSTRIP_SCHEMA_VERSION,
        "filmstrip",
        migrate_filmstrip_v1,
    )
}

#[cfg(test)]
pub fn ensure_wave_schema(conn: &Connection) -> rusqlite::Result<()> {
    ensure_schema(conn, WAVE_SCHEMA_VERSION, "wave", migrate_wave_v1)
}

fn ensure_schema(
    conn: &Connection,
    supported_version: i64,
    db_role: &str,
    migrate_v1: fn(&Connection) -> rusqlite::Result<()>,
) -> rusqlite::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > supported_version {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "IngestQNC {db_role} DB version {version} is newer than supported version {supported_version}"
        )));
    }

    if version < 1 {
        migrate_v1(conn)?;
    }

    Ok(())
}

pub fn upsert_source_identity(
    registry: &Connection,
    record: &SourceIdentityRecord,
) -> rusqlite::Result<()> {
    registry.execute(
        "
        INSERT INTO ingestqnc_sources (
            source_identity,
            source_kind,
            display_name,
            transport_uri,
            identity_evidence_json,
            fallback_fingerprint,
            first_seen_at,
            last_seen_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
        ON CONFLICT(source_identity) DO UPDATE SET
            source_kind = excluded.source_kind,
            display_name = excluded.display_name,
            transport_uri = excluded.transport_uri,
            identity_evidence_json = excluded.identity_evidence_json,
            fallback_fingerprint = excluded.fallback_fingerprint,
            last_seen_at = excluded.last_seen_at
        ",
        params![
            record.source_identity,
            record.source_kind,
            record.display_name,
            record.transport_uri,
            record.identity_evidence_json,
            record.fallback_fingerprint,
            record.seen_at
        ],
    )?;

    registry.execute(
        "
        INSERT INTO ingestqnc_source_locations (
            location_id,
            source_identity,
            transport_uri,
            location_kind,
            first_seen_at,
            last_seen_at,
            evidence_json
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)
        ON CONFLICT(source_identity, transport_uri) DO UPDATE SET
            location_kind = excluded.location_kind,
            last_seen_at = excluded.last_seen_at,
            evidence_json = excluded.evidence_json
        ",
        params![
            stable_location_id(&record.source_identity, &record.transport_uri),
            record.source_identity,
            record.transport_uri,
            record.source_kind,
            record.seen_at,
            record.identity_evidence_json
        ],
    )?;

    Ok(())
}

pub fn upsert_source_module_database(
    registry: &Connection,
    record: &SourceModuleDatabaseRecord,
) -> rusqlite::Result<()> {
    registry.execute(
        "
        INSERT INTO ingestqnc_source_module_databases (
            source_identity,
            module_name,
            database_uri,
            module_schema_version,
            evidence_json,
            first_seen_at,
            last_seen_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
        ON CONFLICT(source_identity, module_name) DO UPDATE SET
            database_uri = excluded.database_uri,
            module_schema_version = excluded.module_schema_version,
            evidence_json = excluded.evidence_json,
            last_seen_at = excluded.last_seen_at
        ",
        params![
            record.source_identity,
            record.module_name,
            record.database_uri,
            record.module_schema_version,
            record.evidence_json,
            record.seen_at
        ],
    )?;
    Ok(())
}

pub fn source_identity_exists(
    registry: &Connection,
    source_identity: &str,
) -> rusqlite::Result<bool> {
    registry
        .query_row(
            "
            SELECT 1
            FROM ingestqnc_sources
            WHERE source_identity = ?1
            ",
            params![source_identity],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
}

#[cfg(test)]
pub fn source_module_database_exists(
    registry: &Connection,
    source_identity: &str,
    module_name: &str,
) -> rusqlite::Result<bool> {
    registry
        .query_row(
            "
            SELECT 1
            FROM ingestqnc_source_module_databases
            WHERE source_identity = ?1 AND module_name = ?2
            ",
            params![source_identity, module_name],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
}

pub fn upsert_content_source_identity(
    content_db: &Connection,
    record: &SourceIdentityRecord,
) -> rusqlite::Result<()> {
    content_db.execute(
        "
        INSERT INTO ingestqnc_content_source_meta (
            source_identity,
            source_kind,
            display_name,
            transport_uri,
            identity_evidence_json,
            fallback_fingerprint,
            first_seen_at,
            last_seen_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
        ON CONFLICT(source_identity) DO UPDATE SET
            source_kind = excluded.source_kind,
            display_name = excluded.display_name,
            transport_uri = excluded.transport_uri,
            identity_evidence_json = excluded.identity_evidence_json,
            fallback_fingerprint = excluded.fallback_fingerprint,
            last_seen_at = excluded.last_seen_at
        ",
        params![
            record.source_identity,
            record.source_kind,
            record.display_name,
            record.transport_uri,
            record.identity_evidence_json,
            record.fallback_fingerprint,
            record.seen_at
        ],
    )?;
    Ok(())
}

pub fn content_source_identity_exists(
    content_db: &Connection,
    source_identity: &str,
) -> rusqlite::Result<bool> {
    content_db
        .query_row(
            "
            SELECT 1
            FROM ingestqnc_content_source_meta
            WHERE source_identity = ?1
            ",
            params![source_identity],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
}

pub fn upsert_clip(
    content_db: &Connection,
    record: &ClipDiscoveryRecord,
) -> rusqlite::Result<ClipUpsertResult> {
    let existing_clip_id: Option<String> = content_db
        .query_row(
            "
            SELECT clip_id
            FROM ingestqnc_clips
            WHERE source_identity = ?1 AND clip_fingerprint = ?2
            ",
            params![record.source_identity, record.clip_fingerprint],
            |row| row.get(0),
        )
        .optional()?;

    let clip_id = existing_clip_id
        .clone()
        .unwrap_or_else(|| stable_clip_id(&record.source_identity, &record.clip_fingerprint));

    if existing_clip_id.is_some() {
        content_db.execute(
            "
            UPDATE ingestqnc_clips
            SET relative_path = ?2,
                original_name = ?3,
                extension = ?4,
                poster_relative_path = COALESCE(?5, poster_relative_path),
                poster_source = COALESCE(?6, poster_source),
                file_size_bytes = ?7,
                clip_created_at = COALESCE(?8, clip_created_at),
                clip_created_at_source = CASE
                    WHEN ?8 IS NULL THEN clip_created_at_source
                    ELSE ?9
                END,
                clip_created_at_offset = COALESCE(?10, clip_created_at_offset),
                timestamp_evidence_json = ?11,
                last_seen_at = ?12,
                deleted_at = NULL
            WHERE clip_id = ?1
            ",
            params![
                clip_id,
                record.relative_path,
                record.original_name,
                record.extension,
                record.poster_relative_path,
                record.poster_source,
                record.file_size_bytes,
                record.clip_created_at,
                record.clip_created_at_source,
                record.clip_created_at_offset,
                record.timestamp_evidence_json,
                record.seen_at
            ],
        )?;
        Ok(ClipUpsertResult {
            clip_id,
            kind: UpsertKind::Updated,
        })
    } else {
        content_db.execute(
            "
            INSERT INTO ingestqnc_clips (
                clip_id,
                source_identity,
                clip_fingerprint,
                relative_path,
                original_name,
                extension,
                poster_relative_path,
                poster_source,
                file_size_bytes,
                clip_created_at,
                clip_created_at_source,
                clip_created_at_offset,
                timestamp_evidence_json,
                discovered_at,
                last_seen_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)
            ",
            params![
                clip_id,
                record.source_identity,
                record.clip_fingerprint,
                record.relative_path,
                record.original_name,
                record.extension,
                record.poster_relative_path,
                record.poster_source,
                record.file_size_bytes,
                record.clip_created_at,
                record.clip_created_at_source,
                record.clip_created_at_offset,
                record.timestamp_evidence_json,
                record.seen_at
            ],
        )?;
        Ok(ClipUpsertResult {
            clip_id,
            kind: UpsertKind::Inserted,
        })
    }
}

pub fn upsert_media_probe_snapshot(
    content_db: &Connection,
    record: &MediaProbeSnapshotRecord,
) -> rusqlite::Result<()> {
    content_db.execute(
        "
        INSERT INTO ingestqnc_media_probe_snapshots (
            probe_id,
            clip_id,
            probe_version,
            probed_at,
            status,
            duration_sec,
            video_codec,
            audio_codec,
            width,
            height,
            fps_num,
            fps_den,
            raw_probe_json,
            error
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
        ON CONFLICT(clip_id, probe_version) DO UPDATE SET
            probe_id = excluded.probe_id,
            probed_at = excluded.probed_at,
            status = excluded.status,
            duration_sec = excluded.duration_sec,
            video_codec = excluded.video_codec,
            audio_codec = excluded.audio_codec,
            width = excluded.width,
            height = excluded.height,
            fps_num = excluded.fps_num,
            fps_den = excluded.fps_den,
            raw_probe_json = excluded.raw_probe_json,
            error = excluded.error
        ",
        params![
            record.probe_id,
            record.clip_id,
            record.probe_version,
            record.probed_at,
            record.status,
            record.duration_sec,
            record.video_codec,
            record.audio_codec,
            record.width,
            record.height,
            record.fps_num,
            record.fps_den,
            record.raw_probe_json,
            record.error
        ],
    )?;
    Ok(())
}

pub fn update_clip_creation_timestamp(
    content_db: &Connection,
    record: &ClipCreationTimestampUpdate,
) -> rusqlite::Result<()> {
    content_db.execute(
        "
        UPDATE ingestqnc_clips
        SET clip_created_at = ?2,
            clip_created_at_source = ?3,
            clip_created_at_offset = ?4,
            timestamp_evidence_json = ?5
        WHERE clip_id = ?1
        ",
        params![
            record.clip_id,
            record.clip_created_at,
            record.clip_created_at_source,
            record.clip_created_at_offset,
            record.timestamp_evidence_json
        ],
    )?;
    Ok(())
}

#[cfg(test)]
pub fn latest_probe_for_clip(
    content_db: &Connection,
    clip_id: &str,
) -> rusqlite::Result<Option<ProbeSnapshotSummary>> {
    content_db
        .query_row(
            "
            SELECT probe_id, clip_id, probe_version, status, duration_sec, width, height
            FROM ingestqnc_media_probe_snapshots
            WHERE clip_id = ?1
            ORDER BY probed_at DESC, probe_id DESC
            LIMIT 1
            ",
            params![clip_id],
            |row| {
                Ok(ProbeSnapshotSummary {
                    probe_id: row.get(0)?,
                    clip_id: row.get(1)?,
                    probe_version: row.get(2)?,
                    status: row.get(3)?,
                    duration_sec: row.get(4)?,
                    width: row.get(5)?,
                    height: row.get(6)?,
                })
            },
        )
        .optional()
}

pub fn stable_clip_id(source_identity: &str, clip_fingerprint: &str) -> String {
    format!(
        "clip:{}:{}",
        normalize_id_part(source_identity),
        normalize_id_part(clip_fingerprint)
    )
}

pub fn stable_location_id(source_identity: &str, transport_uri: &str) -> String {
    format!(
        "location:{}:{}",
        normalize_id_part(source_identity),
        normalize_id_part(transport_uri)
    )
}

fn normalize_id_part(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':') {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn migrate_registry_v1(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(&format!(
        "
        BEGIN;
        PRAGMA application_id = {REGISTRY_APPLICATION_ID};

        CREATE TABLE IF NOT EXISTS ingestqnc_registry_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ingestqnc_sources (
            source_identity TEXT PRIMARY KEY,
            source_kind TEXT NOT NULL CHECK (
                source_kind IN (
                    'local_card',
                    'local_volume',
                    'lan_share',
                    'intranet_gateway',
                    'unknown'
                )
            ),
            display_name TEXT NOT NULL,
            transport_uri TEXT NOT NULL,
            identity_evidence_json TEXT NOT NULL DEFAULT '{{}}',
            fallback_fingerprint TEXT,
            first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
        );

        CREATE TABLE IF NOT EXISTS ingestqnc_source_locations (
            location_id TEXT PRIMARY KEY,
            source_identity TEXT NOT NULL REFERENCES ingestqnc_sources(source_identity) ON DELETE CASCADE,
            transport_uri TEXT NOT NULL,
            location_kind TEXT NOT NULL,
            machine_hint TEXT,
            os_hint TEXT,
            accessible INTEGER NOT NULL DEFAULT 1 CHECK (accessible IN (0, 1)),
            first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            evidence_json TEXT NOT NULL DEFAULT '{{}}',
            UNIQUE(source_identity, transport_uri)
        );

        CREATE TABLE IF NOT EXISTS ingestqnc_source_module_databases (
            source_identity TEXT NOT NULL REFERENCES ingestqnc_sources(source_identity) ON DELETE CASCADE,
            module_name TEXT NOT NULL,
            database_uri TEXT NOT NULL,
            module_schema_version INTEGER NOT NULL,
            evidence_json TEXT NOT NULL DEFAULT '{{}}',
            first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            PRIMARY KEY(source_identity, module_name)
        );

        CREATE INDEX IF NOT EXISTS idx_ingestqnc_locations_source
            ON ingestqnc_source_locations(source_identity);
        CREATE INDEX IF NOT EXISTS idx_ingestqnc_module_databases_source
            ON ingestqnc_source_module_databases(source_identity);

        INSERT OR REPLACE INTO ingestqnc_registry_meta(key, value)
            VALUES ('schema_version', '{REGISTRY_SCHEMA_VERSION}');
        PRAGMA user_version = {REGISTRY_SCHEMA_VERSION};
        COMMIT;
        "
    ))
}

fn migrate_content_v1(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(&format!(
        "
        BEGIN;
        PRAGMA application_id = {CONTENT_APPLICATION_ID};

        CREATE TABLE IF NOT EXISTS ingestqnc_content_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ingestqnc_content_source_meta (
            source_identity TEXT PRIMARY KEY,
            source_kind TEXT NOT NULL CHECK (
                source_kind IN (
                    'local_card',
                    'local_volume',
                    'lan_share',
                    'intranet_gateway',
                    'unknown'
                )
            ),
            display_name TEXT NOT NULL,
            transport_uri TEXT NOT NULL,
            identity_evidence_json TEXT NOT NULL DEFAULT '{{}}',
            fallback_fingerprint TEXT,
            first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
        );

        CREATE TABLE IF NOT EXISTS ingestqnc_clips (
            clip_id TEXT PRIMARY KEY,
            source_identity TEXT NOT NULL REFERENCES ingestqnc_content_source_meta(source_identity) ON DELETE CASCADE,
            clip_fingerprint TEXT NOT NULL,
            relative_path TEXT NOT NULL,
            original_name TEXT NOT NULL,
            extension TEXT,
            poster_relative_path TEXT,
            poster_source TEXT,
            file_size_bytes INTEGER,
            clip_created_at TEXT,
            clip_created_at_source TEXT NOT NULL DEFAULT 'unknown' CHECK (
                clip_created_at_source IN (
                    'embedded_camera',
                    'embedded_container',
                    'filesystem_created',
                    'filesystem_modified',
                    'manual',
                    'unknown'
                )
            ),
            clip_created_at_offset TEXT,
            timestamp_evidence_json TEXT NOT NULL DEFAULT '{{}}',
            discovered_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            deleted_at TEXT,
            UNIQUE(source_identity, clip_fingerprint)
        );

        CREATE TABLE IF NOT EXISTS ingestqnc_media_probe_snapshots (
            probe_id TEXT PRIMARY KEY,
            clip_id TEXT NOT NULL REFERENCES ingestqnc_clips(clip_id) ON DELETE CASCADE,
            probe_version TEXT NOT NULL,
            probed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            status TEXT NOT NULL CHECK (status IN ('ok', 'error')),
            duration_sec REAL,
            video_codec TEXT,
            audio_codec TEXT,
            width INTEGER,
            height INTEGER,
            fps_num INTEGER,
            fps_den INTEGER,
            raw_probe_json TEXT NOT NULL,
            error TEXT,
            UNIQUE(clip_id, probe_version)
        );

        CREATE INDEX IF NOT EXISTS idx_ingestqnc_clips_source
            ON ingestqnc_clips(source_identity, relative_path);
        CREATE INDEX IF NOT EXISTS idx_ingestqnc_probe_clip_time
            ON ingestqnc_media_probe_snapshots(clip_id, probed_at DESC);

        CREATE VIEW IF NOT EXISTS ingestqnc_catalog_read_v1 AS
        SELECT
            c.clip_id,
            c.source_identity,
            c.clip_fingerprint,
            c.relative_path,
            c.original_name,
            c.extension,
            c.poster_relative_path,
            c.poster_source,
            c.file_size_bytes,
            c.clip_created_at,
            c.clip_created_at_source,
            c.clip_created_at_offset,
            c.timestamp_evidence_json,
            s.source_kind,
            s.display_name AS source_display_name,
            s.transport_uri,
            p.probe_id AS latest_probe_id,
            p.probe_version AS latest_probe_version,
            p.status AS latest_probe_status,
            p.duration_sec,
            p.video_codec,
            p.audio_codec,
            p.width,
            p.height,
            p.fps_num,
            p.fps_den
        FROM ingestqnc_clips c
        JOIN ingestqnc_content_source_meta s ON s.source_identity = c.source_identity
        LEFT JOIN ingestqnc_media_probe_snapshots p
            ON p.probe_id = (
                SELECT p2.probe_id
                FROM ingestqnc_media_probe_snapshots p2
                WHERE p2.clip_id = c.clip_id
                ORDER BY p2.probed_at DESC, p2.probe_id DESC
                LIMIT 1
            )
        WHERE c.deleted_at IS NULL;

        INSERT OR REPLACE INTO ingestqnc_content_meta(key, value)
            VALUES ('schema_version', '{CONTENT_SCHEMA_VERSION}');
        INSERT OR REPLACE INTO ingestqnc_content_meta(key, value)
            VALUES ('module_name', '{MODULE_CONTENT}');
        PRAGMA user_version = {CONTENT_SCHEMA_VERSION};
        COMMIT;
        "
    ))
}

fn migrate_content_v2(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(&format!(
        "
        BEGIN;
        ALTER TABLE ingestqnc_clips ADD COLUMN poster_relative_path TEXT;
        ALTER TABLE ingestqnc_clips ADD COLUMN poster_source TEXT;
        DROP VIEW IF EXISTS ingestqnc_catalog_read_v1;

        CREATE VIEW ingestqnc_catalog_read_v1 AS
        SELECT
            c.clip_id,
            c.source_identity,
            c.clip_fingerprint,
            c.relative_path,
            c.original_name,
            c.extension,
            c.poster_relative_path,
            c.poster_source,
            c.file_size_bytes,
            c.clip_created_at,
            c.clip_created_at_source,
            c.clip_created_at_offset,
            c.timestamp_evidence_json,
            s.source_kind,
            s.display_name AS source_display_name,
            s.transport_uri,
            p.probe_id AS latest_probe_id,
            p.probe_version AS latest_probe_version,
            p.status AS latest_probe_status,
            p.duration_sec,
            p.video_codec,
            p.audio_codec,
            p.width,
            p.height,
            p.fps_num,
            p.fps_den
        FROM ingestqnc_clips c
        JOIN ingestqnc_content_source_meta s ON s.source_identity = c.source_identity
        LEFT JOIN ingestqnc_media_probe_snapshots p
            ON p.probe_id = (
                SELECT p2.probe_id
                FROM ingestqnc_media_probe_snapshots p2
                WHERE p2.clip_id = c.clip_id
                ORDER BY p2.probed_at DESC, p2.probe_id DESC
                LIMIT 1
            )
        WHERE c.deleted_at IS NULL;

        INSERT OR REPLACE INTO ingestqnc_content_meta(key, value)
            VALUES ('schema_version', '{CONTENT_SCHEMA_VERSION}');
        PRAGMA user_version = {CONTENT_SCHEMA_VERSION};
        COMMIT;
        "
    ))
}

#[cfg(test)]
fn migrate_filmstrip_v1(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(&format!(
        "
        BEGIN;
        PRAGMA application_id = {FILMSTRIP_APPLICATION_ID};

        CREATE TABLE IF NOT EXISTS ingestqnc_filmstrip_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ingestqnc_filmstrip_sources (
            source_identity TEXT PRIMARY KEY,
            content_database_uri TEXT NOT NULL,
            evidence_json TEXT NOT NULL DEFAULT '{{}}',
            first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
        );

        CREATE TABLE IF NOT EXISTS ingestqnc_posters (
            poster_id TEXT PRIMARY KEY,
            source_identity TEXT NOT NULL,
            clip_id TEXT NOT NULL,
            seek_sec REAL NOT NULL,
            width INTEGER,
            height INTEGER,
            artifact_uri TEXT NOT NULL,
            content_hash TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            UNIQUE(source_identity, clip_id, seek_sec, artifact_uri)
        );

        CREATE TABLE IF NOT EXISTS ingestqnc_filmstrip_frames (
            frame_id TEXT PRIMARY KEY,
            source_identity TEXT NOT NULL,
            clip_id TEXT NOT NULL,
            frame_index INTEGER NOT NULL,
            seek_sec REAL NOT NULL,
            poster_id TEXT,
            artifact_uri TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            UNIQUE(source_identity, clip_id, frame_index)
        );

        CREATE INDEX IF NOT EXISTS idx_ingestqnc_posters_clip
            ON ingestqnc_posters(source_identity, clip_id);
        CREATE INDEX IF NOT EXISTS idx_ingestqnc_filmstrip_clip
            ON ingestqnc_filmstrip_frames(source_identity, clip_id, frame_index);

        CREATE VIEW IF NOT EXISTS ingestqnc_filmstrip_read_v1 AS
        SELECT
            frame_id,
            source_identity,
            clip_id,
            frame_index,
            seek_sec,
            poster_id,
            artifact_uri,
            created_at
        FROM ingestqnc_filmstrip_frames;

        INSERT OR REPLACE INTO ingestqnc_filmstrip_meta(key, value)
            VALUES ('schema_version', '{FILMSTRIP_SCHEMA_VERSION}');
        INSERT OR REPLACE INTO ingestqnc_filmstrip_meta(key, value)
            VALUES ('module_name', '{MODULE_FILMSTRIP}');
        PRAGMA user_version = {FILMSTRIP_SCHEMA_VERSION};
        COMMIT;
        "
    ))
}

#[cfg(test)]
fn migrate_wave_v1(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(&format!(
        "
        BEGIN;
        PRAGMA application_id = {WAVE_APPLICATION_ID};

        CREATE TABLE IF NOT EXISTS ingestqnc_wave_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ingestqnc_wave_sources (
            source_identity TEXT PRIMARY KEY,
            content_database_uri TEXT NOT NULL,
            evidence_json TEXT NOT NULL DEFAULT '{{}}',
            first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
        );

        CREATE TABLE IF NOT EXISTS ingestqnc_waveforms (
            waveform_id TEXT PRIMARY KEY,
            source_identity TEXT NOT NULL,
            clip_id TEXT NOT NULL,
            channel_layout TEXT NOT NULL DEFAULT 'unknown',
            sample_rate INTEGER,
            peaks_uri TEXT,
            peaks_json TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            UNIQUE(source_identity, clip_id, channel_layout, sample_rate)
        );

        CREATE INDEX IF NOT EXISTS idx_ingestqnc_waveforms_clip
            ON ingestqnc_waveforms(source_identity, clip_id);

        CREATE VIEW IF NOT EXISTS ingestqnc_wave_read_v1 AS
        SELECT
            waveform_id,
            source_identity,
            clip_id,
            channel_layout,
            sample_rate,
            peaks_uri,
            peaks_json,
            created_at
        FROM ingestqnc_waveforms;

        INSERT OR REPLACE INTO ingestqnc_wave_meta(key, value)
            VALUES ('schema_version', '{WAVE_SCHEMA_VERSION}');
        INSERT OR REPLACE INTO ingestqnc_wave_meta(key, value)
            VALUES ('module_name', '{MODULE_WAVE}');
        PRAGMA user_version = {WAVE_SCHEMA_VERSION};
        COMMIT;
        "
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    fn source(seen_at: &str) -> SourceIdentityRecord {
        SourceIdentityRecord {
            source_identity: "card:media_card_a:sn-001".into(),
            source_kind: "local_card".into(),
            display_name: "MEDIA_CARD_A".into(),
            transport_uri: "qnc+local://localhost/card/sn-001".into(),
            identity_evidence_json: r#"{"serial":"SN-001"}"#.into(),
            fallback_fingerprint: Some("fallback-a".into()),
            seen_at: seen_at.into(),
        }
    }

    fn module_database(
        module_name: &str,
        schema_version: i64,
        seen_at: &str,
    ) -> SourceModuleDatabaseRecord {
        SourceModuleDatabaseRecord {
            source_identity: "card:media_card_a:sn-001".into(),
            module_name: module_name.into(),
            database_uri: format!(
                "qnc+local://localhost/ingest-db/card_media_card_a_sn-001/{module_name}.sqlite"
            ),
            module_schema_version: schema_version,
            evidence_json: format!(r#"{{"module":"{module_name}"}}"#),
            seen_at: seen_at.into(),
        }
    }

    fn clip(fingerprint: &str, seen_at: &str) -> ClipDiscoveryRecord {
        ClipDiscoveryRecord {
            source_identity: "card:media_card_a:sn-001".into(),
            clip_fingerprint: fingerprint.into(),
            relative_path: "DCIM/100QNC/A001_C001.mov".into(),
            original_name: "A001_C001.mov".into(),
            extension: Some("mov".into()),
            poster_relative_path: None,
            poster_source: None,
            file_size_bytes: Some(42_000_000),
            clip_created_at: Some("2026-09-03T09:14:22Z".into()),
            clip_created_at_source: "embedded_container".into(),
            clip_created_at_offset: Some("+00:00".into()),
            timestamp_evidence_json: r#"{"format_tags":{"creation_time":"2026-09-03T09:14:22Z"}}"#
                .into(),
            seen_at: seen_at.into(),
        }
    }

    #[test]
    fn registry_schema_keeps_only_sources_locations_and_module_db_index() -> rusqlite::Result<()> {
        let conn = open_registry_in_memory()?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        assert_eq!(version, REGISTRY_SCHEMA_VERSION);

        for table in [
            "ingestqnc_registry_meta",
            "ingestqnc_sources",
            "ingestqnc_source_locations",
            "ingestqnc_source_module_databases",
        ] {
            assert!(object_exists(&conn, "table", table)?, "{table} missing");
        }
        assert!(!object_exists(&conn, "table", "ingestqnc_clips")?);
        assert!(!object_exists(
            &conn,
            "table",
            "ingestqnc_media_probe_snapshots"
        )?);
        assert!(!object_exists(
            &conn,
            "table",
            "ingestqnc_filmstrip_frames"
        )?);
        assert!(!object_exists(&conn, "table", "ingestqnc_waveforms")?);
        Ok(())
    }

    #[test]
    fn content_schema_keeps_clip_and_probe_data_only() -> rusqlite::Result<()> {
        let conn = open_content_in_memory()?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        assert_eq!(version, CONTENT_SCHEMA_VERSION);

        for table in [
            "ingestqnc_content_meta",
            "ingestqnc_content_source_meta",
            "ingestqnc_clips",
            "ingestqnc_media_probe_snapshots",
        ] {
            assert!(object_exists(&conn, "table", table)?, "{table} missing");
        }
        assert!(object_exists(&conn, "view", "ingestqnc_catalog_read_v1")?);
        assert!(!object_exists(
            &conn,
            "table",
            "ingestqnc_source_module_databases"
        )?);
        assert!(!object_exists(
            &conn,
            "table",
            "ingestqnc_filmstrip_frames"
        )?);
        assert!(!object_exists(&conn, "table", "ingestqnc_waveforms")?);
        Ok(())
    }

    #[test]
    fn filmstrip_schema_is_a_separate_artifact_module() -> rusqlite::Result<()> {
        let conn = open_filmstrip_in_memory()?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        assert_eq!(version, FILMSTRIP_SCHEMA_VERSION);

        for table in [
            "ingestqnc_filmstrip_meta",
            "ingestqnc_filmstrip_sources",
            "ingestqnc_posters",
            "ingestqnc_filmstrip_frames",
        ] {
            assert!(object_exists(&conn, "table", table)?, "{table} missing");
        }
        assert!(object_exists(&conn, "view", "ingestqnc_filmstrip_read_v1")?);
        assert!(!object_exists(&conn, "table", "ingestqnc_clips")?);
        assert!(!object_exists(&conn, "table", "ingestqnc_waveforms")?);
        Ok(())
    }

    #[test]
    fn wave_schema_is_a_separate_artifact_module() -> rusqlite::Result<()> {
        let conn = open_wave_in_memory()?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        assert_eq!(version, WAVE_SCHEMA_VERSION);

        for table in [
            "ingestqnc_wave_meta",
            "ingestqnc_wave_sources",
            "ingestqnc_waveforms",
        ] {
            assert!(object_exists(&conn, "table", table)?, "{table} missing");
        }
        assert!(object_exists(&conn, "view", "ingestqnc_wave_read_v1")?);
        assert!(!object_exists(&conn, "table", "ingestqnc_clips")?);
        assert!(!object_exists(
            &conn,
            "table",
            "ingestqnc_filmstrip_frames"
        )?);
        Ok(())
    }

    #[test]
    fn open_modular_databases_create_separate_schema_files() -> rusqlite::Result<()> {
        let registry_path = unique_temp_db_path("registry");
        let content_path = unique_temp_db_path("content");
        let filmstrip_path = unique_temp_db_path("filmstrip");
        let wave_path = unique_temp_db_path("wave");

        let registry = open_registry_database(&registry_path)?;
        let content = open_content_database(&content_path)?;
        let filmstrip = open_filmstrip_database(&filmstrip_path)?;
        let wave = open_wave_database(&wave_path)?;

        let registry_version: i64 =
            registry.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let content_version: i64 =
            content.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let filmstrip_version: i64 =
            filmstrip.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let wave_version: i64 = wave.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        drop(registry);
        drop(content);
        drop(filmstrip);
        drop(wave);

        let files_exist = registry_path.exists()
            && content_path.exists()
            && filmstrip_path.exists()
            && wave_path.exists();
        fs::remove_file(&registry_path).ok();
        fs::remove_file(&content_path).ok();
        fs::remove_file(&filmstrip_path).ok();
        fs::remove_file(&wave_path).ok();

        assert_eq!(registry_version, REGISTRY_SCHEMA_VERSION);
        assert_eq!(content_version, CONTENT_SCHEMA_VERSION);
        assert_eq!(filmstrip_version, FILMSTRIP_SCHEMA_VERSION);
        assert_eq!(wave_version, WAVE_SCHEMA_VERSION);
        assert!(files_exist);
        Ok(())
    }

    #[test]
    fn source_identity_upsert_updates_registry_last_seen_without_duplicate() -> rusqlite::Result<()>
    {
        let registry = open_registry_in_memory()?;
        upsert_source_identity(&registry, &source("2026-09-03T09:00:00Z"))?;

        let mut seen_again = source("2026-09-03T10:00:00Z");
        seen_again.display_name = "MEDIA_CARD_A_RENAMED".into();
        upsert_source_identity(&registry, &seen_again)?;

        let count: i64 =
            registry.query_row("SELECT COUNT(*) FROM ingestqnc_sources", [], |row| {
                row.get(0)
            })?;
        let (display_name, first_seen, last_seen): (String, String, String) = registry.query_row(
            "
                SELECT display_name, first_seen_at, last_seen_at
                FROM ingestqnc_sources
                WHERE source_identity = ?1
                ",
            params!["card:media_card_a:sn-001"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

        assert_eq!(count, 1);
        assert_eq!(display_name, "MEDIA_CARD_A_RENAMED");
        assert_eq!(first_seen, "2026-09-03T09:00:00Z");
        assert_eq!(last_seen, "2026-09-03T10:00:00Z");
        Ok(())
    }

    #[test]
    fn registry_tracks_multiple_module_databases_per_source() -> rusqlite::Result<()> {
        let registry = open_registry_in_memory()?;
        upsert_source_identity(&registry, &source("2026-09-03T09:00:00Z"))?;
        upsert_source_module_database(
            &registry,
            &module_database(
                MODULE_CONTENT,
                CONTENT_SCHEMA_VERSION,
                "2026-09-03T09:01:00Z",
            ),
        )?;
        upsert_source_module_database(
            &registry,
            &module_database(
                MODULE_FILMSTRIP,
                FILMSTRIP_SCHEMA_VERSION,
                "2026-09-03T09:02:00Z",
            ),
        )?;
        upsert_source_module_database(
            &registry,
            &module_database(MODULE_WAVE, WAVE_SCHEMA_VERSION, "2026-09-03T09:03:00Z"),
        )?;

        let mut moved_wave =
            module_database(MODULE_WAVE, WAVE_SCHEMA_VERSION, "2026-09-03T10:00:00Z");
        moved_wave.database_uri =
            "qnc+lan://storage/ingest-db/card_media_card_a_sn-001/wave.sqlite".into();
        upsert_source_module_database(&registry, &moved_wave)?;

        let count: i64 = registry.query_row(
            "SELECT COUNT(*) FROM ingestqnc_source_module_databases WHERE source_identity = ?1",
            params!["card:media_card_a:sn-001"],
            |row| row.get(0),
        )?;
        let wave_row: (String, String) = registry.query_row(
            "
            SELECT database_uri, last_seen_at
            FROM ingestqnc_source_module_databases
            WHERE source_identity = ?1 AND module_name = ?2
            ",
            params!["card:media_card_a:sn-001", MODULE_WAVE],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        assert_eq!(count, 3);
        assert_eq!(
            wave_row.0,
            "qnc+lan://storage/ingest-db/card_media_card_a_sn-001/wave.sqlite"
        );
        assert_eq!(wave_row.1, "2026-09-03T10:00:00Z");
        assert!(source_module_database_exists(
            &registry,
            "card:media_card_a:sn-001",
            MODULE_FILMSTRIP
        )?);
        Ok(())
    }

    #[test]
    fn content_source_identity_is_required_before_clip_rows() -> rusqlite::Result<()> {
        let content_db = open_content_in_memory()?;
        assert!(!content_source_identity_exists(
            &content_db,
            "card:media_card_a:sn-001"
        )?);

        let err = upsert_clip(
            &content_db,
            &clip("fingerprint-001", "2026-09-03T09:01:00Z"),
        )
        .expect_err("content DB must be initialized for source before clips");
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));

        upsert_content_source_identity(&content_db, &source("2026-09-03T09:00:00Z"))?;
        assert!(content_source_identity_exists(
            &content_db,
            "card:media_card_a:sn-001"
        )?);
        Ok(())
    }

    #[test]
    fn clip_uniqueness_is_source_identity_plus_clip_fingerprint() -> rusqlite::Result<()> {
        let content_db = open_content_in_memory()?;
        upsert_content_source_identity(&content_db, &source("2026-09-03T09:00:00Z"))?;

        let inserted = upsert_clip(
            &content_db,
            &clip("fingerprint-001", "2026-09-03T09:01:00Z"),
        )?;
        let mut rediscovered = clip("fingerprint-001", "2026-09-03T11:00:00Z");
        rediscovered.relative_path = "DCIM/101QNC/A001_C001.mov".into();
        let updated = upsert_clip(&content_db, &rediscovered)?;

        assert_eq!(inserted.clip_id, updated.clip_id);
        assert_eq!(inserted.kind, UpsertKind::Inserted);
        assert_eq!(updated.kind, UpsertKind::Updated);

        let count: i64 =
            content_db.query_row("SELECT COUNT(*) FROM ingestqnc_clips", [], |row| row.get(0))?;
        let (relative_path, last_seen): (String, String) = content_db.query_row(
            "SELECT relative_path, last_seen_at FROM ingestqnc_clips WHERE clip_id = ?1",
            params![inserted.clip_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(count, 1);
        assert_eq!(relative_path, "DCIM/101QNC/A001_C001.mov");
        assert_eq!(last_seen, "2026-09-03T11:00:00Z");
        Ok(())
    }

    #[test]
    fn same_clip_fingerprint_can_exist_in_another_content_database() -> rusqlite::Result<()> {
        let content_a = open_content_in_memory()?;
        upsert_content_source_identity(&content_a, &source("2026-09-03T09:00:00Z"))?;

        let content_b = open_content_in_memory()?;
        let mut second_source = source("2026-09-03T09:00:00Z");
        second_source.source_identity = "lan:nas-qnc:news".into();
        second_source.source_kind = "lan_share".into();
        second_source.transport_uri = "qnc+lan://nas-qnc/news".into();
        upsert_content_source_identity(&content_b, &second_source)?;

        upsert_clip(&content_a, &clip("fingerprint-001", "2026-09-03T09:01:00Z"))?;
        let mut second_clip = clip("fingerprint-001", "2026-09-03T09:02:00Z");
        second_clip.source_identity = "lan:nas-qnc:news".into();
        upsert_clip(&content_b, &second_clip)?;

        assert_eq!(clip_count(&content_a), 1);
        assert_eq!(clip_count(&content_b), 1);
        Ok(())
    }

    #[test]
    fn source_identity_exists_reads_registry_only() -> rusqlite::Result<()> {
        let registry = open_registry_in_memory()?;
        assert!(!source_identity_exists(
            &registry,
            "card:media_card_a:sn-001"
        )?);

        upsert_source_identity(&registry, &source("2026-09-03T09:00:00Z"))?;

        assert!(source_identity_exists(
            &registry,
            "card:media_card_a:sn-001"
        )?);
        assert!(!source_identity_exists(&registry, "card:other")?);
        Ok(())
    }

    #[test]
    fn latest_probe_is_read_from_content_database() -> rusqlite::Result<()> {
        let content_db = open_content_in_memory()?;
        upsert_content_source_identity(&content_db, &source("2026-09-03T09:00:00Z"))?;
        let clip_id = upsert_clip(
            &content_db,
            &clip("fingerprint-001", "2026-09-03T09:01:00Z"),
        )?
        .clip_id;

        upsert_media_probe_snapshot(
            &content_db,
            &MediaProbeSnapshotRecord {
                probe_id: "probe-1".into(),
                clip_id: clip_id.clone(),
                probe_version: "ffprobe-full-v1".into(),
                probed_at: "2026-09-03T09:02:00Z".into(),
                status: "ok".into(),
                duration_sec: Some(12.2),
                video_codec: Some("h264".into()),
                audio_codec: Some("aac".into()),
                width: Some(1920),
                height: Some(1080),
                fps_num: Some(25),
                fps_den: Some(1),
                raw_probe_json: r#"{"streams":[]}"#.into(),
                error: None,
            },
        )?;

        let probe = latest_probe_for_clip(&content_db, &clip_id)?.expect("missing probe");
        assert_eq!(probe.probe_id, "probe-1");
        assert_eq!(probe.status, "ok");
        assert_eq!(probe.duration_sec, Some(12.2));
        assert_eq!(probe.width, Some(1920));
        Ok(())
    }

    #[test]
    fn catalog_read_view_exposes_content_database_rows() -> rusqlite::Result<()> {
        let content_db = open_content_in_memory()?;
        upsert_content_source_identity(&content_db, &source("2026-09-03T09:00:00Z"))?;
        let clip_id = upsert_clip(
            &content_db,
            &clip("fingerprint-001", "2026-09-03T09:01:00Z"),
        )?
        .clip_id;
        upsert_media_probe_snapshot(
            &content_db,
            &MediaProbeSnapshotRecord {
                probe_id: "probe-read-view".into(),
                clip_id: clip_id.clone(),
                probe_version: "ffprobe-full-v1".into(),
                probed_at: "2026-09-03T09:02:00Z".into(),
                status: "ok".into(),
                duration_sec: Some(12.2),
                video_codec: Some("h264".into()),
                audio_codec: Some("aac".into()),
                width: Some(3840),
                height: Some(2160),
                fps_num: Some(50),
                fps_den: Some(1),
                raw_probe_json: r#"{"format":{"duration":"12.2"}}"#.into(),
                error: None,
            },
        )?;

        let row: (String, String, String, Option<f64>, Option<i64>, String) = content_db
            .query_row(
                "
                SELECT
                    source_identity,
                    clip_fingerprint,
                    clip_created_at,
                    duration_sec,
                    width,
                    transport_uri
                FROM ingestqnc_catalog_read_v1
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
                        row.get(5)?,
                    ))
                },
            )?;

        assert_eq!(row.0, "card:media_card_a:sn-001");
        assert_eq!(row.1, "fingerprint-001");
        assert_eq!(row.2, "2026-09-03T09:14:22Z");
        assert_eq!(row.3, Some(12.2));
        assert_eq!(row.4, Some(3840));
        assert_eq!(row.5, "qnc+local://localhost/card/sn-001");
        Ok(())
    }

    #[test]
    fn future_module_names_do_not_require_registry_schema_changes() -> rusqlite::Result<()> {
        let registry = open_registry_in_memory()?;
        upsert_source_identity(&registry, &source("2026-09-03T09:00:00Z"))?;
        upsert_source_module_database(
            &registry,
            &module_database("transcript", 1, "2026-09-03T09:04:00Z"),
        )?;

        assert!(source_module_database_exists(
            &registry,
            "card:media_card_a:sn-001",
            "transcript"
        )?);
        Ok(())
    }

    fn clip_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM ingestqnc_clips", [], |row| row.get(0))
            .unwrap()
    }

    fn object_exists(conn: &Connection, kind: &str, name: &str) -> rusqlite::Result<bool> {
        conn.query_row(
            "
            SELECT 1
            FROM sqlite_master
            WHERE type = ?1 AND name = ?2
            ",
            params![kind, name],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
    }

    fn unique_temp_db_path(kind: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ingestqnc-{kind}-{nanos}.sqlite"))
    }
}
