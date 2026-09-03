# IngestQNC - upute za agenta

Workspace: this IngestQNC project root.

IngestQNC is a standalone closed ingest application in the QNC application
family. It is not a helper module inside any other QNC application.

Locked product boundary:

- A user must be able to launch only IngestQNC.
- IngestQNC must not require another QNC application, an editing project, Story
  state, timeline state or ingest-select state.
- Other QNC applications do not have to exist on the same computer as
  IngestQNC.
- The stable IngestQNC modular database contract is the only product consumed by
  other QNC applications.
- Other QNC applications may store only their own references to IngestQNC DB
  rows; they must not duplicate IngestQNC source catalog, probe, poster,
  poster-filmstrip, waveform, source identity or physical media location data.
- Other QNC applications may read the IngestQNC database contract only. They
  must not call IngestQNC scanner, probe, artifact generation or source
  detection routines.
- IngestQNC must not run as a long-lived service for other applications.
- After an ingest job completes, IngestQNC shuts down cleanly; no
  background ingest service or hidden listener may remain active.

IngestQNC database rule:

- IngestQNC uses one database technology: relational SQLite databases.
- Database modularity means separate physical SQLite files by role, not a mix of
  unrelated database systems.
- The registry DB owns source/card/share/gateway identity, source locations and
  source module database URIs.
- The content DB owns clip catalog and full MediaProbe snapshots for a selected
  source.
- The filmstrip DB owns poster and posted-picture filmstrip rows for a selected
  source.
- The wave DB owns waveform rows for a selected source.
- Future routines may add their own module DBs by registering a new
  `module_name`, without changing existing registry/content/filmstrip/wave
  schemas.
- Every persisted database location must be a QNC transport URI that can resolve
  on laptop, LAN/NAS or intranet.

IngestQNC ownership:

- Source clip catalog.
- Source/card/share/gateway identity.
- Full MediaProbe snapshots.
- Poster/thumbnail records.
- Poster-filmstrip display data.
- Waveform data.
- Physical media location records for laptop, LAN/NAS and intranet/gateway.

Simplicity rule:

- IngestQNC must stay simple for the operator.
- The main workflow is: choose source, click `Odaberi`, identify source/card,
  write source/card timestamps, scan clips, run probe, generate display
  artifacts, show status.
- Do not expose project, edit, story, timeline, render or ingest-select concepts
  from other applications in the IngestQNC operator workflow.
- Do not add hidden automation paths that can scan, probe or mutate the database
  outside the visible ingest workflow.
- Do not add service APIs for other applications to trigger ingest work.
- Prefer one clear source state and one clear clip state over parallel states or
  duplicated caches.

Source identity:

- Before clip scanning, identify the source as durable `source_identity`.
- Prefer card/device/media serial number or volume/filesystem UUID.
- Store raw identity evidence and fallback fingerprint evidence.
- Clip uniqueness is `source_identity` plus stable `clip_fingerprint`.
- Discovery is idempotent: seeing the same source updates `last_seen`; it must
  not duplicate clips.

Clip metadata:

- Persist `clip_created_at` when available.
- Prefer embedded camera/container `creation_time` or recorded-at metadata.
- Filesystem created/birth/modified time is fallback evidence only.
- Store raw timestamp evidence and source/offset information.

Probe rule:

- MediaProbe is a full source probe and writes to the IngestQNC database.
- Later workflows must read persisted probe data from the database and must not
  run new probe as fallback.

Platform rule:

- Required OS targets: Windows, Linux, macOS.
- Required CPU families: x86/x86_64 and ARM/ARM64/aarch64.
- Runtime paths and persisted transport paths must be OS-neutral QNC transport
  URIs. Raw OS paths may exist only as operator input or evidence, never as
  stable `source_identity`, `transport_uri`, `database_uri` or external read
  data.
- If one shared CPU runtime complicates implementation, two kernels are allowed:
  `kernel_x86` and `kernel_arm`.
- Separate kernels are implementation/package boundaries only; they must share
  the same database schema, stable IDs, transport contracts and conformance
  tests.

UI rule:

- Preserve the existing native egui Ingest UI as the visual baseline.
- Do not redesign UI as part of the split.
- Passive UI components may be copied/extracted.
- Active workflow code from other QNC applications must not be copied into
  IngestQNC without explicit responsibility separation.
