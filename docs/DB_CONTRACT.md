# IngestQNC Database Contract

IngestQNC uses one database technology: relational SQLite databases. The
contract is modular by physical database file and schema role.

The database set is the product of an ingest run. After IngestQNC finishes its
ingest work, it shuts down. Other QNC applications may be started afterwards
and may read stable references from these databases, but must not duplicate
IngestQNC source catalog, probe, poster-filmstrip, wave or physical location
rows into their own databases.

Other QNC applications read this database contract only. They must not use
IngestQNC as a scanner/probe/artifact service, and they must not trigger ingest
routines through a hidden API, daemon or background listener.

## Modular Database Roles

- Registry DB: global list of sources/cards/shares/gateways, source locations
  and module database URIs.
- Content DB: clip catalog and full MediaProbe snapshots for one selected
  source.
- Filmstrip DB: poster and posted-picture filmstrip records for one selected
  source.
- Wave DB: waveform records for one selected source.
- Future module DBs: allowed by adding a new `module_name` row in the registry,
  without changing existing registry/content/filmstrip/wave schemas.

All stable source locations and module database locations use QNC transport
URIs, so the same contract works on a laptop, local LAN/NAS and intranet:

- `qnc+local://localhost/card/<serial>`
- `qnc+local://localhost/volume/<uuid>`
- `qnc+local://localhost/fallback/<signature>`
- `qnc+lan://server/share/...`
- `qnc+intranet://gateway/...`

Raw Windows, Linux, macOS, UNC or mount paths may exist only as operator input
or evidence JSON. They must not be stored as stable `source_identity`,
`transport_uri`, `database_uri`, clip path or external read data.

Runtime may use `INGESTQNC_HOME` as the local working root for SQLite files and
`INGESTQNC_DB_URI_BASE` as the persisted QNC URI base. If no URI base is
provided, the local default is `qnc+local://localhost/ingest-db`. For LAN and
intranet deployment, `INGESTQNC_DB_URI_BASE` must be a `qnc+lan://...` or
`qnc+intranet://...` base URI.

`INGESTQNC_INITIAL_SOURCE` is allowed only as a runtime/UI startup input for
manual launch and live testing. It may contain the current operator access path
or a QNC transport URI, but it must not become a stable database location unless
it is already a valid QNC transport URI.

Runtime source access is separate from persisted identity. `INGESTQNC_LAN_MOUNT_ROOT`
may map `qnc+lan://server/share/...` to a local mounted path, and
`INGESTQNC_INTRANET_MOUNT_ROOT` may map `qnc+intranet://gateway/...` to a local
mounted path. On Windows, `qnc+lan://server/share/...` may also resolve through
a UNC path. These access paths are runtime-only and remain evidence, not stable
database contract values.

`INGESTQNC_FFPROBE` may point to an OS-specific ffprobe executable for the
active machine. That path is runtime configuration only; it is not part of the
database contract.

## Versions

- Registry DB: SQLite `user_version` `1`, application id `0x514E4352`,
  metadata table `ingestqnc_registry_meta`.
- Content DB: SQLite `user_version` `2`, application id `0x514E4343`,
  metadata table `ingestqnc_content_meta`.
- Filmstrip DB: SQLite `user_version` `1`, application id `0x514E4346`,
  metadata table `ingestqnc_filmstrip_meta`.
- Wave DB: SQLite `user_version` `1`, application id `0x514E4357`, metadata
  table `ingestqnc_wave_meta`.

## Registry DB

- `ingestqnc_sources`: durable source/card/share/gateway identities.
- `ingestqnc_source_locations`: laptop, LAN/NAS and intranet/gateway locations
  for a source identity.
- `ingestqnc_source_module_databases`: module database index keyed by
  `source_identity + module_name`.

The registry DB must not contain clips, probe snapshots, filmstrip frames or
waveform rows.

## Content DB

- `ingestqnc_content_source_meta`: source identity copied into the selected
  source content database before clip rows are written.
- `ingestqnc_clips`: source clip catalog, keyed by
  `source_identity + clip_fingerprint`.
- `ingestqnc_media_probe_snapshots`: full MediaProbe output persisted by
  IngestQNC.
- `ingestqnc_catalog_read_v1`: stable read view for clip identity, source
  identity, transport URI, clip creation metadata and latest probe summary.

`ingestqnc_media_probe_snapshots.raw_probe_json` stores the full ffprobe JSON
snapshot. Summary columns such as duration, codec, frame size and fps are only
read conveniences. Embedded camera/container creation timestamps discovered by
MediaProbe update `ingestqnc_clips.clip_created_at` and override filesystem
fallback evidence.

The content DB must not contain filmstrip or wave artifact rows.

## Filmstrip DB

- `ingestqnc_filmstrip_sources`: source and linked content DB URI for the
  filmstrip module.
- `ingestqnc_posters`: poster/thumbnail artifact records.
- `ingestqnc_filmstrip_frames`: posted-picture filmstrip display frames.
- `ingestqnc_filmstrip_read_v1`: stable read view for filmstrip rows.

## Wave DB

- `ingestqnc_wave_sources`: source and linked content DB URI for the wave
  module.
- `ingestqnc_waveforms`: waveform artifact records.
- `ingestqnc_wave_read_v1`: stable read view for waveform rows.

## Identity Rules

- IngestQNC must identify a source before scanning clips.
- Preferred evidence: card/device/media serial or volume/filesystem UUID.
- Local disk/card serial and volume/display name are persisted as source
  identity evidence in the registry DB and copied into
  `ingestqnc_content_source_meta` before clip/probe rows are written.
- Fallback evidence is stored, but does not replace the durable
  `source_identity`.
- Rediscovery of the same source updates `last_seen_at`; it must not duplicate
  rows.
- Clip uniqueness is enforced by `UNIQUE(source_identity, clip_fingerprint)` in
  the content DB.

## Timestamp Rules

- `ingestqnc_clips.clip_created_at` stores the clip creation timestamp when
  available.
- `clip_created_at_source` records the evidence type:
  `embedded_camera`, `embedded_container`, `filesystem_created`,
  `filesystem_modified`, `manual` or `unknown`.
- Raw timestamp evidence is stored in `timestamp_evidence_json`.
- Filesystem timestamps are fallback evidence only. MediaProbe later writes
  embedded camera/container evidence into the content DB.

## Read Rule

The read views are read-only from the perspective of other QNC applications.
External application state should store only references to stable IDs such as
`clip_id`, `source_identity + clip_fingerprint` and module artifact IDs, not
copied source/probe/artifact rows.

The database contract is not an ingest command surface. Other applications must
not start source detection, scanning, MediaProbe or visual artifact generation
through any database or hidden IngestQNC process.
