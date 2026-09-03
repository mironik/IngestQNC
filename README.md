# IngestQNC

Standalone native ingest application for the QNC application family.

IngestQNC is not a helper module, background service or API for another QNC
application. The operator workflow is intentionally simple: choose a source,
identify the source, scan clips, run MediaProbe, create display artifacts and
write the result to modular SQLite databases.

## Product boundary

- IngestQNC can be launched without QNC.app.
- IngestQNC is the only application that scans sources, detects source identity,
  runs MediaProbe and creates ingest artifacts.
- Other QNC applications read only the IngestQNC database contract.
- Other QNC applications must not call scanner, probe, source identity or
  artifact generation routines.
- After ingest completes, no background ingest service or listener remains
  active.

## Runtime targets

- Windows, Linux and macOS.
- x86/x86_64 and ARM/ARM64/aarch64 CPU families.
- Laptop, local LAN and intranet deployments.
- Persisted paths use OS-neutral QNC transport URIs. Raw OS paths are only
  runtime operator input or evidence.

## Databases

IngestQNC uses relational SQLite databases split by role:

- registry database: source/card/share/gateway identity and module database URIs
- content database: clip catalog and full MediaProbe snapshots
- filmstrip database: poster and posted-picture filmstrip rows
- wave database: waveform rows

Runtime database files are local generated data and are ignored by Git.

## Development

```powershell
cargo check
cargo test
cargo run --bin IngestQNC
```
