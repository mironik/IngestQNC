# IngestQNC Source Identity

IngestQNC identifies the source before scanning clips. Scanning and MediaProbe
must not start until a durable source identity has been created or an explicit
fallback identity has been recorded.

## Transport URIs

IngestQNC stores transport-oriented locations, not raw OS paths, as the primary
runtime contract:

- `qnc+local://localhost/card/<serial>` for directly attached card media.
- `qnc+local://localhost/volume/<uuid>` for directly attached volume media.
- `qnc+local://localhost/fallback/<signature>` when no stronger local evidence
  exists.
- `qnc+lan://server/share/...` for LAN/NAS sources.
- `qnc+intranet://gateway/...` for routed intranet sources.

Raw OS paths and discovery evidence stay only in `identity_evidence_json`.
For directly attached card media, the durable `source_identity` is
`card:<serial>`. Volume labels, display names and operator hints are descriptive
metadata only; they must not be part of the durable identity.

## Evidence Order

1. Explicit `.qnc-source-identity.json` marker in the source root.
2. Card/device/media serial number.
3. Volume/filesystem UUID.
4. Root signature fallback from volume label and top-level camera layout.

Fallback identity is allowed only when it can be made from OS-neutral source
evidence such as volume label and top-level camera layout. If the only available
evidence is a raw OS path, local source identity fails and scan must not start.

## Marker File

Supported marker names:

- `.qnc-source-identity.json`
- `qnc-source-identity.json`
- `.qnc_card_identity.json`
- `QNC_SOURCE_IDENTITY.json`

Supported fields:

- `source_identity`
- `source_kind`
- `display_name`
- `transport_uri`
- `media_serial`, `card_serial`, `device_serial` or `serial`
- `volume_uuid` or `filesystem_uuid`
- `volume_label`

## Database Rule

The source identity adapter writes only the registry DB tables
`ingestqnc_sources` and `ingestqnc_source_locations`.

When a source is selected for ingest, the selected content DB must be initialized
with `ingestqnc_content_source_meta` before clip scan writes `ingestqnc_clips`.
MediaProbe writes to the content DB. Posters and filmstrip data write to the
filmstrip DB. Waveform generation writes to the wave DB.
