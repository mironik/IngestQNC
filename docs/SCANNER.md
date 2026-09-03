# IngestQNC Scanner

The scanner is an IngestQNC runtime component. It discovers source media files
only after the source has already been recorded in the registry DB
`ingestqnc_sources` table and initialized in the selected content DB
`ingestqnc_content_source_meta` table.

It must stay a simple visible step in the ingest workflow: one confirmed source
goes in, clip catalog rows come out, then MediaProbe runs as part of the same
visible ingest procedure.

The scanner is compiled into the app only because it is connected to the visible
source confirmation/refresh workflow. It must not become a hidden service or
background ingest routine.

## Rules

- The scanner must receive a known `source_identity` from the registry DB before
  walking files.
- The scanner writes clip rows only to the selected content DB.
- The scanner must not write clips, probe snapshots or artifacts into the
  registry DB.
- The scanner belongs only to the active IngestQNC ingest workflow.
- Other applications must not call the scanner as a service.
- No scanner background worker may remain active after the ingest job is done.
- The operator access root is runtime-only. It can be a local mount, card reader
  path or OS-specific filesystem path, but it is not a stable database path.
- LAN/Intranet QNC URIs must resolve through a runtime mount or UNC bridge
  before walking files.
- Persisted clip paths are slash-separated relative paths inside the identified
  source.
- Raw Windows, Linux, macOS, UNC or mount paths must not be persisted as
  `relative_path`, `transport_uri`, `source_identity` or external read data.
- Discovery writes clip rows keyed by `source_identity + clip_fingerprint`.
- Rediscovery updates `last_seen_at` and must not duplicate existing clips.
- Scanner timestamp data is filesystem fallback evidence only.
- Embedded camera/container creation timestamps belong to the later MediaProbe
  phase, which writes full probe snapshots into the content DB.

## Fingerprint

The first scanner fingerprint is `content-sample-sha256-v1`. It hashes the file
size and sampled file bytes. It intentionally does not include the operator
access root, so the same source can be rediscovered from a different OS mount
without creating duplicate clip rows.
