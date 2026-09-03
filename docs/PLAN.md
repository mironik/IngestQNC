# IngestQNC Plan

Canonical workspace: this IngestQNC project root.

## Product Rule - Simple Ingest

- IngestQNC must stay a simple operator tool.
- The visible workflow is: choose source, click `Odaberi`, identify the source,
  scan clips, run probe, generate poster/filmstrip/waveform artifacts and show
  status.
- Keep project, story, timeline, edit, render and ingest-select concepts from
  other applications out of the IngestQNC workflow.
- Each phase must add one visible ingest capability, not a parallel project
  system.
- Other QNC applications read only the IngestQNC modular database contract.
- Do not expose scanner, probe, source identity or artifact generation routines
  as a service for other applications.
- IngestQNC is not a daemon. After ingest completes, it must be able to exit
  without leaving background ingest work active.
- The output of IngestQNC is a set of relational SQLite databases. After that
  output is complete, another QNC application may start and read it.

## Phase 1 - Standalone UI Shell

Status: complete.

- Native Rust/egui application named `IngestQNC`.
- Preserve the existing QNC Ingest visual structure.
- Keep source browser, clip grid, preview monitor, source dock, poster-filmstrip and waveform display areas.
- No active external project workflow, host API, import queue or MediaProbe
  execution in this phase.

## Phase 2 - Modular Ingest Databases

Status: registry + content runtime creation wired to UI; filmstrip/wave schemas
are contract-tested and remain inactive until their visible phases.

- Add standalone IngestQNC SQLite schemas and migrations.
- Use one database technology, but split physical database roles:
  registry, content, filmstrip and wave.
- Registry owns source/card/share/gateway identity, source locations and module
  database URIs.
- Content owns clip catalog and full MediaProbe snapshots for a selected source.
- Filmstrip owns poster and posted-picture filmstrip rows for a selected source.
- Wave owns waveform rows for a selected source.
- New routines may add new module databases through registry rows without
  changing existing module schemas.
- Store `clip_created_at`, raw timestamp evidence, `source_identity` and `clip_fingerprint`.
- Add migration and idempotency tests before adding runtime discovery.

## Phase 3 - Source Identity

Status: adapter implemented and connected to the UI source confirmation step;
Windows volume serial/label evidence is read when available, and other
OS-native providers must feed the same database contract.

- Detect source identity before scanning clips.
- Prefer device/card/media serial or volume/filesystem UUID.
- Support laptop, LAN/NAS and intranet/gateway sources through the transport protocol.
- Keep stable paths OS-neutral; raw OS paths are evidence only.
- Update `last_seen` for known sources without duplicating clips.
- UI confirmation creates/opens the registry DB and selected source content DB,
  then registers the content module with a QNC `database_uri`.

## Phase 4 - Discovery And Probe

Status: the visible `Odaberi` action identifies the source, writes source/card
timestamps to the registry/content DBs, scans discovered content and runs
MediaProbe for all discovered clips. The lower `Ingest` action remains a manual
rerun for selected clips.

- Add source scanner inside IngestQNC, not another QNC application.
- Scanner requires a known `source_identity` in the registry DB and initialized
  source metadata in the selected content DB before walking files.
- Runtime access roots are never persisted as stable paths.
- Persist only slash-separated relative clip paths under the identified source.
- LAN and intranet QNC URIs must resolve through a configured runtime access
  mount or UNC bridge before scanner walks files.
- Add full MediaProbe execution and persist probe snapshots in the content DB.
- Later workflows read probe data from the DB and must not run probe again.

## Phase 5 - Visual Artifacts

- Generate and persist poster-filmstrip data in the filmstrip DB.
- Generate and persist waveform data in the wave DB.
- The UI reads these artifacts from IngestQNC state.
- Filmstrip remains a posted-picture display, not playback logic.

## Phase 6 - Database Product

- Keep the stable modular database contract as the only product of an ingest
  run.
- Other QNC applications read existing IngestQNC DB module rows only after
  IngestQNC has finished and shut down.
- Other QNC applications may store their own selection references to IngestQNC
  DB rows.
- Other QNC applications must not trigger IngestQNC ingest routines.

## Phase 7 - Packaging

- Package for Windows, Linux and macOS.
- Support x86/x86_64 and ARM/ARM64/aarch64 CPU families.
- If needed, split CPU-specific implementation into `kernel_x86` and `kernel_arm` with one shared schema and one shared transport contract.
