# IngestQNC Live Test

This live test uses a real card, mounted folder, LAN share or intranet source.
It does not create fake source clips and it does not write to the source. It
writes only to the configured IngestQNC SQLite database home.

## Windows

Insert or mount the real card/source, then run:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\run-live-test.ps1 -Source "E:\"
```

For a LAN share reachable by UNC:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\run-live-test.ps1 -Source "\\nas-qnc\cards\A001" -DbUriBase "qnc+lan://nas-qnc/ingest-db/live"
```

For an intranet QNC transport URI, provide the runtime mount root:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\run-live-test.ps1 -Source "qnc+intranet://gateway/cards/A001" -IntranetMountRoot "Z:\mounted-intranet" -DbUriBase "qnc+intranet://gateway/ingest-db/live"
```

## UI Steps

1. Verify that the source shown in the UI is the real card/source.
2. Click `Odaberi`.
3. Confirm that the clip grid contains the real media files.
4. Confirm that the status shows scan and MediaProbe results.
5. Optional: select one or more clips and click `Ingest` to rerun probe for
   only those clips.
6. Close IngestQNC when finished.

The launcher waits until the UI closes, then cleans the temporary Cargo target
unless `-KeepBuild` is passed.

## Runtime Inputs

- `INGESTQNC_HOME`: local filesystem directory for SQLite files.
- `INGESTQNC_DB_URI_BASE`: persisted OS-neutral QNC URI base for database rows.
- `INGESTQNC_INITIAL_SOURCE`: runtime-only operator source to open at startup.
- `INGESTQNC_LAN_MOUNT_ROOT`: optional runtime mapping for `qnc+lan://`.
- `INGESTQNC_INTRANET_MOUNT_ROOT`: optional runtime mapping for
  `qnc+intranet://`.
- `INGESTQNC_FFPROBE`: optional path/name for the ffprobe executable. If unset,
  IngestQNC uses `ffprobe` from `PATH`.

Raw OS paths are runtime access evidence only. Persisted database locations and
stable source locations remain QNC transport URIs.
