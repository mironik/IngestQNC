use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{self, TextureHandle};

use crate::{
    model::{AppState, ClipCard},
    poster_loader::PosterAssetLoader,
    runtime::{
        IdentifiedSourceRuntime, ProbeRuntimeReport, ProbedSourceRuntime, RuntimeController,
        ScannedSourceRuntime,
    },
    ui::{
        self,
        filmstrip_background::FilmFrame,
        shell::{self, ShellCommand},
    },
};

const CLIP_POLL_INTERVAL: Duration = Duration::from_millis(1500);
const POSTER_REQUEST_LIMIT: usize = 16;
const FILMSTRIP_FRAME_COUNT: usize = 14;

pub struct IngestQncApp {
    state: AppState,
    runtime: RuntimeController,
    event_tx: Sender<RuntimeEvent>,
    event_rx: Receiver<RuntimeEvent>,
    source_busy: bool,
    probe_busy: bool,
    last_clip_poll: Option<Instant>,
    last_clip_fingerprint: Option<u64>,
    poster_loader: PosterAssetLoader,
    poster_textures: HashMap<String, TextureHandle>,
    poster_texture_paths: HashMap<String, String>,
    poster_failures: HashSet<String>,
    film_frames: Vec<FilmFrame>,
}

impl IngestQncApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::theme::set_active(&cc.egui_ctx, ui::theme::ThemeId::Dark);
        ui::theme::apply_app_fonts(&cc.egui_ctx);

        let runtime = RuntimeController::from_environment();
        let (event_tx, event_rx) = mpsc::channel();
        let mut state = AppState::new();
        if let Some(initial_source) = std::env::var_os("INGESTQNC_INITIAL_SOURCE") {
            state
                .source
                .open_location(initial_source.to_string_lossy().to_string());
        }
        refresh_source_entries(&runtime, &mut state);

        Self {
            state,
            runtime,
            event_tx,
            event_rx,
            source_busy: false,
            probe_busy: false,
            last_clip_poll: None,
            last_clip_fingerprint: None,
            poster_loader: PosterAssetLoader::new(),
            poster_textures: HashMap::new(),
            poster_texture_paths: HashMap::new(),
            poster_failures: HashSet::new(),
            film_frames: Vec::new(),
        }
    }
}

impl eframe::App for IngestQncApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ui::theme::set_active(ctx, self.state.theme);
        ui::theme::apply_egui_visuals(ctx, &self.state.theme.tokens());
        self.drain_runtime_events(ctx);
        self.poll_clip_snapshot_if_due(ctx);
        self.pump_posters(ctx);

        let command = shell::show(
            ctx,
            &self.runtime,
            &mut self.state,
            &self.poster_textures,
            &self.film_frames,
        );
        self.handle_shell_command(command, ctx);

        if self.state.ingest_busy || self.state.probe_busy || self.poster_loader.has_pending() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
}

#[derive(Debug)]
enum RuntimeEvent {
    SourceIdentified(Result<IdentifiedSourceRuntime, String>),
    SourceScanned(Result<ScannedSourceRuntime, String>),
    ProbeStarted(usize),
    ProbeFinished(Result<ProbedSourceRuntime, String>),
}

impl IngestQncApp {
    fn handle_shell_command(&mut self, command: ShellCommand, ctx: &egui::Context) {
        match command {
            ShellCommand::None => {}
            ShellCommand::IngestSelectedSource => self.spawn_full_ingest(ctx),
            ShellCommand::ScanSelectedSource => self.spawn_rescan(ctx),
            ShellCommand::ProbeSelectedClips => self.spawn_selected_probe(ctx),
        }
    }

    fn spawn_full_ingest(&mut self, ctx: &egui::Context) {
        if self.state.ingest_busy || self.state.probe_busy {
            return;
        }
        let location = self.state.source.path.trim().to_owned();
        if location.is_empty() {
            self.state.status_line = "Odaberi lokaciju izvora.".into();
            return;
        }

        self.state.ingest_busy = true;
        self.state.source.busy = true;
        self.source_busy = true;
        self.probe_busy = false;
        self.state.probe_busy = false;
        self.state.identified_source = None;
        self.state.clips.clear();
        self.state.active_clip_id = None;
        self.clear_poster_cache();
        self.last_clip_poll = None;
        self.last_clip_fingerprint = None;
        self.state.status_line = "Identifikacija izvora...".into();

        let runtime = self.runtime.clone();
        let tx = self.event_tx.clone();
        thread::spawn(move || {
            let identified = match runtime.identify_selected_source(&location) {
                Ok(identified) => {
                    let _ = tx.send(RuntimeEvent::SourceIdentified(Ok(identified.clone())));
                    identified
                }
                Err(error) => {
                    let _ = tx.send(RuntimeEvent::SourceIdentified(Err(error.to_string())));
                    return;
                }
            };

            let scanned = match runtime
                .scan_identified_source(&identified.source_identity, &identified.operator_location)
            {
                Ok(scanned) => {
                    let _ = tx.send(RuntimeEvent::SourceScanned(Ok(scanned.clone())));
                    scanned
                }
                Err(error) => {
                    let _ = tx.send(RuntimeEvent::SourceScanned(Err(error.to_string())));
                    return;
                }
            };

            let clip_ids = scanned
                .clips
                .iter()
                .map(|clip| clip.id.clone())
                .collect::<Vec<_>>();
            if clip_ids.is_empty() {
                let _ = tx.send(RuntimeEvent::ProbeFinished(Ok(ProbedSourceRuntime {
                    report: ProbeRuntimeReport {
                        source_identity: identified.source_identity,
                        requested_clips: 0,
                        candidate_clips: 0,
                        probes_ok: 0,
                        probes_error: 0,
                        probes_skipped: 0,
                    },
                    clips: scanned.clips,
                })));
                return;
            }

            let _ = tx.send(RuntimeEvent::ProbeStarted(clip_ids.len()));
            let probed = runtime
                .probe_selected_clips(
                    &identified.source_identity,
                    &identified.operator_location,
                    clip_ids,
                )
                .map_err(|error| error.to_string());
            let _ = tx.send(RuntimeEvent::ProbeFinished(probed));
        });
        ctx.request_repaint();
    }

    fn spawn_rescan(&mut self, ctx: &egui::Context) {
        if self.state.ingest_busy || self.state.probe_busy {
            return;
        }
        let Some(source) = self.state.identified_source.clone() else {
            self.state.status_line = "Prvo odaberi izvor.".into();
            return;
        };

        self.state.ingest_busy = true;
        self.state.source.busy = true;
        self.source_busy = true;
        self.state.status_line = "Skeniranje izvora...".into();

        let runtime = self.runtime.clone();
        let tx = self.event_tx.clone();
        thread::spawn(move || {
            let scanned = runtime
                .scan_identified_source(&source.source_identity, &source.operator_location)
                .map_err(|error| error.to_string());
            let _ = tx.send(RuntimeEvent::SourceScanned(scanned));
        });
        ctx.request_repaint();
    }

    fn spawn_selected_probe(&mut self, ctx: &egui::Context) {
        if self.state.probe_busy || self.state.ingest_busy {
            return;
        }
        let Some(source) = self.state.identified_source.clone() else {
            self.state.status_line = "Prvo odaberi izvor.".into();
            return;
        };
        let clip_ids = self.state.selected_clip_ids();
        if clip_ids.is_empty() {
            self.state.status_line = "Nema odabranih klipova za Ingest.".into();
            return;
        }

        self.state.ingest_busy = true;
        self.state.probe_busy = true;
        self.probe_busy = true;
        self.last_clip_poll = None;
        self.state.status_line = format!("Probe pokrenut za {} klipova...", clip_ids.len());

        let runtime = self.runtime.clone();
        let tx = self.event_tx.clone();
        thread::spawn(move || {
            let _ = tx.send(RuntimeEvent::ProbeStarted(clip_ids.len()));
            let probed = runtime
                .probe_selected_clips(&source.source_identity, &source.operator_location, clip_ids)
                .map_err(|error| error.to_string());
            let _ = tx.send(RuntimeEvent::ProbeFinished(probed));
        });
        ctx.request_repaint();
    }

    fn drain_runtime_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                RuntimeEvent::SourceIdentified(result) => match result {
                    Ok(identified) => {
                        self.state.identified_source =
                            Some(shell::identified_source_summary(&identified));
                        self.state.status_line = format!(
                            "Izvor: {} ({})",
                            identified.display_name, identified.source_identity
                        );
                    }
                    Err(error) => {
                        self.finish_all_work();
                        self.state.status_line = format!("Identifikacija nije uspjela: {error}");
                    }
                },
                RuntimeEvent::SourceScanned(result) => {
                    self.source_busy = false;
                    self.state.source.busy = false;
                    match result {
                        Ok(scanned) => {
                            let total = scanned.clips.len();
                            self.apply_clip_snapshot(scanned.clips);
                            self.state.status_line = format!(
                                "Skenirano: {} media datoteka, {} klipova u bazi.",
                                scanned.report.media_files_seen, total
                            );
                        }
                        Err(error) => {
                            self.finish_all_work();
                            self.state.status_line = format!("Skeniranje nije uspjelo: {error}");
                        }
                    }
                }
                RuntimeEvent::ProbeStarted(total) => {
                    self.probe_busy = true;
                    self.state.probe_busy = true;
                    self.state.status_line = format!("Probe u tijeku: {total} klipova.");
                }
                RuntimeEvent::ProbeFinished(result) => {
                    self.probe_busy = false;
                    self.state.probe_busy = false;
                    self.state.ingest_busy = false;
                    match result {
                        Ok(probed) => {
                            let ok = probed.report.probes_ok;
                            let err = probed.report.probes_error;
                            self.apply_clip_snapshot(probed.clips);
                            self.state.status_line =
                                format!("Probe završen: {ok} OK, {err} greška.");
                        }
                        Err(error) => {
                            self.state.status_line = format!("Probe nije uspio: {error}");
                        }
                    }
                }
            }
            ctx.request_repaint();
        }
    }

    fn finish_all_work(&mut self) {
        self.source_busy = false;
        self.probe_busy = false;
        self.state.source.busy = false;
        self.state.ingest_busy = false;
        self.state.probe_busy = false;
    }

    fn poll_clip_snapshot_if_due(&mut self, ctx: &egui::Context) {
        if !(self.state.ingest_busy || self.state.probe_busy) {
            return;
        }
        if self
            .last_clip_poll
            .is_some_and(|last| last.elapsed() < CLIP_POLL_INTERVAL)
        {
            return;
        }
        self.last_clip_poll = Some(Instant::now());

        let Some(source) = self.state.identified_source.clone() else {
            return;
        };
        if let Ok(clips) = self
            .runtime
            .load_source_clips(&source.source_identity, &source.operator_location)
        {
            self.apply_clip_snapshot(clips);
            ctx.request_repaint();
        }
    }

    fn apply_clip_snapshot(&mut self, clips: Vec<ClipCard>) {
        let fingerprint = clip_snapshot_fingerprint(&clips);
        if self.last_clip_fingerprint == Some(fingerprint) {
            return;
        }
        self.state.replace_clips_preserving_selection(clips);
        self.last_clip_fingerprint = Some(fingerprint);
    }

    fn pump_posters(&mut self, ctx: &egui::Context) {
        for result in self.poster_loader.poll() {
            match result.image {
                Ok(image) => {
                    let texture = ctx.load_texture(
                        format!("ingestqnc-poster-{}", result.clip_id),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.poster_texture_paths
                        .insert(result.clip_id.clone(), result.path);
                    self.poster_textures.insert(result.clip_id, texture);
                }
                Err(_) => {
                    self.poster_failures
                        .insert(poster_failure_key(&result.clip_id, &result.path));
                }
            }
        }

        let mut requested = 0usize;
        for clip in
            prioritized_poster_clips(&self.state.clips, self.state.active_clip_id.as_deref())
        {
            if requested >= POSTER_REQUEST_LIMIT {
                break;
            }
            let Some(path) = clip.poster_access_path.as_deref() else {
                continue;
            };
            if self
                .poster_texture_paths
                .get(&clip.id)
                .is_some_and(|loaded| loaded == path)
                || self.poster_loader.is_pending(&clip.id)
                || self
                    .poster_failures
                    .contains(&poster_failure_key(&clip.id, path))
            {
                continue;
            }
            if self.poster_loader.request(&clip.id, path) {
                requested += 1;
            }
        }

        self.rebuild_filmstrip_frames();
        if requested > 0 || self.poster_loader.has_pending() {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }

    fn rebuild_filmstrip_frames(&mut self) {
        let Some(active_id) = self.state.active_clip_id.as_deref() else {
            self.film_frames.clear();
            return;
        };
        let Some(texture) = self.poster_textures.get(active_id).cloned() else {
            self.film_frames.clear();
            return;
        };
        let url = self
            .state
            .active_clip()
            .and_then(|clip| clip.poster_relative_path.clone())
            .unwrap_or_default();
        self.film_frames = (0..FILMSTRIP_FRAME_COUNT)
            .map(|index| FilmFrame {
                index: index as i64,
                seek_sec: index as f64,
                url: url.clone(),
                texture: Some(texture.clone()),
                load_attempts: 0,
            })
            .collect();
    }

    fn clear_poster_cache(&mut self) {
        self.poster_textures.clear();
        self.poster_texture_paths.clear();
        self.poster_failures.clear();
        self.film_frames.clear();
    }
}

fn prioritized_poster_clips<'a>(
    clips: &'a [ClipCard],
    active_id: Option<&str>,
) -> Vec<&'a ClipCard> {
    let mut out = Vec::with_capacity(clips.len());
    if let Some(active_id) = active_id {
        if let Some(active) = clips.iter().find(|clip| clip.id == active_id) {
            out.push(active);
        }
    }
    for clip in clips {
        if Some(clip.id.as_str()) != active_id {
            out.push(clip);
        }
    }
    out
}

fn clip_snapshot_fingerprint(clips: &[ClipCard]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for clip in clips {
        clip.id.hash(&mut hasher);
        clip.name.hash(&mut hasher);
        clip.duration_sec.to_bits().hash(&mut hasher);
        clip.ingest_status.hash(&mut hasher);
        clip.clip_created_at.hash(&mut hasher);
        clip.poster_relative_path.hash(&mut hasher);
        clip.poster_source.hash(&mut hasher);
    }
    hasher.finish()
}

fn poster_failure_key(clip_id: &str, path: &str) -> String {
    format!("{clip_id}\n{path}")
}

fn refresh_source_entries(runtime: &RuntimeController, state: &mut AppState) {
    match runtime.list_source_entries(state.source.kind, &state.source.path) {
        Ok(snapshot) => {
            state.source.roots = snapshot.roots;
            state.source.path = snapshot.path;
            state.source.parent = snapshot.parent;
            state.source.entries = snapshot.entries;
            state.source.error = None;
            state.source.busy = false;
        }
        Err(error) => {
            state.source.entries.clear();
            state.source.error = Some(error.to_string());
            state.source.busy = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_source_entries_keeps_runtime_errors_in_source_state() {
        let runtime = RuntimeController::from_environment();
        let mut state = AppState::new();
        state.source.open_path("?:\\missing".into());
        refresh_source_entries(&runtime, &mut state);

        assert!(state.source.entries.is_empty());
        assert!(!state.source.busy);
    }
}
