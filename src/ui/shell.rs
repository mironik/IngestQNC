use std::collections::HashMap;

use eframe::egui::{self, RichText, TextureHandle, Vec2};

use crate::{
    model::{
        duration_label, AppState, ClipCard, IdentifiedSourceSummary, IngestLibraryTab, SourceKind,
    },
    runtime::{IdentifiedSourceRuntime, RuntimeController},
    ui::{
        filmstrip_background::FilmFrame,
        layout,
        location_browser::{self, LocationBrowserAction},
        media_card::{self, MediaCardFeatures, MediaCardInput},
        source_dock::{self, SourceDockAction, SourceDockInput},
        theme,
        timeline::TimelineFocusPaint,
        timeline_progress::TimelineProgressModel,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellCommand {
    None,
    IngestSelectedSource,
    ScanSelectedSource,
    ProbeSelectedClips,
}

pub fn show(
    ctx: &egui::Context,
    runtime: &RuntimeController,
    state: &mut AppState,
    poster_textures: &HashMap<String, TextureHandle>,
    film_frames: &[FilmFrame],
) -> ShellCommand {
    let panel_bg = theme::current_ctx(ctx).bg;
    let mut command = ShellCommand::None;

    egui::TopBottomPanel::bottom("footer")
        .exact_height(36.0)
        .frame(egui::Frame::NONE.fill(theme::current_ctx(ctx).surface))
        .show(ctx, |ui| footer(ui, state));

    egui::TopBottomPanel::bottom("workspace_status")
        .exact_height(22.0)
        .frame(egui::Frame::NONE.fill(panel_bg).inner_margin(egui::Margin {
            left: 8,
            right: 8,
            top: 2,
            bottom: 2,
        }))
        .show(ctx, |ui| status_bar(ui, state));

    let dock_h = source_dock::dock_height(state.expanded_audio, true);
    egui::TopBottomPanel::bottom("ingest_source_dock")
        .exact_height(dock_h)
        .frame(egui::Frame::NONE.fill(panel_bg).inner_margin(0.0))
        .show(ctx, |ui| {
            let action = source_dock_panel(ui, state, film_frames);
            set_command(&mut command, dispatch_source_dock_action(state, action));
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(panel_bg))
        .show(ctx, |ui| {
            ingest_form(ui, runtime, state, poster_textures, &mut command)
        });

    command
}

fn footer(ui: &mut egui::Ui, state: &mut AppState) {
    let height = ui.available_height();
    ui.columns(3, |cols| {
        cols[0].with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_height(height);
            ui.spacing_mut().item_spacing.x = 8.0;
            for id in theme::ThemeId::ALL {
                if theme::link_tab(ui, id.label(), state.theme == id).clicked() {
                    state.theme = id;
                }
            }
            ui.separator();
            ui.label(
                RichText::new("IngestQNC")
                    .size(theme::FONT_UI)
                    .color(theme::current(ui).muted),
            );
        });

        cols[1].with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.set_min_height(height);
            ui.horizontal_centered(|ui| {
                let _ = theme::link_tab(ui, "Ingest", true);
            });
        });

        cols[2].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.set_min_height(height);
            ui.label(
                RichText::new(format!("{} klipa", state.clips.len()))
                    .size(theme::FONT_UI)
                    .color(theme::current(ui).muted),
            );
        });
    });
}

fn status_bar(ui: &mut egui::Ui, state: &AppState) {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_height(ui.available_height());
        let status = state.status_line.trim();
        let status = if status.is_empty() { "Ready" } else { status };
        ui.label(
            RichText::new(status)
                .size(theme::FONT_UI)
                .color(theme::current(ui).muted),
        );
    });
}

fn ingest_form(
    ui: &mut egui::Ui,
    runtime: &RuntimeController,
    state: &mut AppState,
    poster_textures: &HashMap<String, TextureHandle>,
    command: &mut ShellCommand,
) {
    layout::editorial_shell(ui, |ui, metrics, side| match side {
        layout::ShellSide::Left => {
            layout::media_column_monitor(
                ui,
                metrics,
                |ui, preview_h| {
                    layout::preview(
                        ui,
                        layout::PreviewInput {
                            height: preview_h,
                            texture: None,
                            empty_label: "Odaberi klip",
                            sense: egui::Sense::hover(),
                        },
                    );
                },
                |ui, _rest| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    pool_head(ui, state);
                    let body_h = ui.available_height().max(0.0);
                    layout::content_panel(ui, body_h, |ui| {
                        source_panel(ui, runtime, state, command)
                    });
                },
            );
        }
        layout::ShellSide::Right => {
            ingest_clip_strip(ui, metrics.height, state, poster_textures);
        }
    });
}

fn pool_head(ui: &mut egui::Ui, state: &mut AppState) {
    theme::chrome_row(ui, true, |ui| {
        for (tab, label) in [
            (IngestLibraryTab::All, "All"),
            (IngestLibraryTab::Virtual, "Virtual"),
        ] {
            if theme::link_tab(ui, label, state.library_tab == tab).clicked() {
                state.library_tab = tab;
            }
            ui.add_space(10.0);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::transport_btn(ui, "]")
                .on_hover_text("Mark OUT")
                .clicked()
            {
                state.status_line = "OUT".into();
            }
            if theme::transport_btn(ui, "[")
                .on_hover_text("Mark IN")
                .clicked()
            {
                state.status_line = "IN".into();
            }
            if theme::transport_btn(ui, ">")
                .on_hover_text("Play / Pause")
                .clicked()
            {
                state.status_line = "Preview playback nije aktivan u samostalnom ingestu.".into();
            }
        });
    });
}

fn source_panel(
    ui: &mut egui::Ui,
    runtime: &RuntimeController,
    state: &mut AppState,
    command: &mut ShellCommand,
) {
    ui.set_min_height(ui.available_height());
    let source_serial = source_serial_suffix(state);
    match location_browser::show(ui, &state.source, source_serial.as_deref()) {
        LocationBrowserAction::None => {}
        LocationBrowserAction::SelectKind(kind) => {
            state.source.kind = kind;
            if matches!(kind, SourceKind::Local) {
                state.source.open_path(String::new());
                refresh_source_entries(runtime, state);
            } else {
                state.source.roots = false;
                state.source.path.clear();
                state.source.parent = None;
                state.source.entries.clear();
                state.source.selected_root_label = None;
                state.source.error = None;
            }
            clear_selected_source(state);
            state.status_line = format!("Stablo: {}", kind.label());
        }
        LocationBrowserAction::OpenPath(path) => {
            clear_selected_source(state);
            if matches!(state.source.kind, SourceKind::Local) {
                state.source.open_path(path.clone());
                refresh_source_entries(runtime, state);
            } else {
                state.source.roots = false;
                state.source.path = path.clone();
                state.source.parent = None;
                state.source.entries.clear();
                state.source.selected_root_label = None;
                state.source.error = None;
            }
            state.status_line = if path.trim().is_empty() {
                "Stablo: računalo.".into()
            } else {
                format!("Stablo: {path}")
            };
        }
        LocationBrowserAction::Confirm => {
            set_command(command, ShellCommand::IngestSelectedSource);
        }
        LocationBrowserAction::Cancel => {
            clear_selected_source(state);
            state.status_line = "Stablo: računalo.".into();
        }
    }
}

fn source_serial_suffix(state: &AppState) -> Option<String> {
    let source = state.identified_source.as_ref()?;
    let value = source.identity_value.trim();
    if value.is_empty() || source.identity_basis == "transport_endpoint" {
        return None;
    }
    let serial = value
        .strip_prefix("windows-volume-serial-")
        .unwrap_or(value)
        .trim();
    (!serial.is_empty()).then(|| serial.to_owned())
}

fn clear_selected_source(state: &mut AppState) {
    state.identified_source = None;
    state.clips.clear();
    state.active_clip_id = None;
}

fn refresh_source_entries(runtime: &RuntimeController, state: &mut AppState) {
    match runtime.list_source_entries(state.source.kind, &state.source.path) {
        Ok(snapshot) => {
            state.source.roots = snapshot.roots;
            state.source.path = snapshot.path;
            state.source.parent = snapshot.parent;
            state.source.entries = snapshot.entries;
            state.source.selected_root_label = snapshot.selected_root_label;
            state.source.error = None;
            state.source.busy = false;
        }
        Err(error) => {
            state.source.entries.clear();
            state.source.selected_root_label = None;
            state.source.error = Some(error.to_string());
            state.source.busy = false;
        }
    }
}

pub fn identified_source_summary(identified: &IdentifiedSourceRuntime) -> IdentifiedSourceSummary {
    IdentifiedSourceSummary {
        operator_location: identified.operator_location.clone(),
        source_identity: identified.source_identity.clone(),
        source_kind: identified.source_kind.clone(),
        display_name: identified.display_name.clone(),
        transport_uri: identified.transport_uri.clone(),
        content_database_uri: identified.content_database_uri.clone(),
        identity_basis: identified.identity_basis.clone(),
        identity_label: identified.identity_label.clone(),
        identity_value: identified.identity_value.clone(),
        confidence: identified.confidence.clone(),
        seen_at: identified.seen_at.clone(),
    }
}

fn ingest_clip_strip(
    ui: &mut egui::Ui,
    height: f32,
    state: &mut AppState,
    poster_textures: &HashMap<String, TextureHandle>,
) {
    let active_id = state.active_clip_id.clone().unwrap_or_default();
    let clips = state.clips.clone();
    layout::content_panel(ui, height, |ui| {
        show_card_grid(
            ui,
            CardGridInput {
                height: ui.available_height().max(0.0),
                selected_id: &active_id,
                clips: &clips,
                poster_textures,
                empty_message: "Nema klipova — lijevo odaberi mapu → U redu.",
            },
            state,
        );
    });
}

struct CardGridInput<'a> {
    height: f32,
    selected_id: &'a str,
    clips: &'a [ClipCard],
    poster_textures: &'a HashMap<String, TextureHandle>,
    empty_message: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CardGridAction {
    Activate(String),
    ToggleSelection(String),
}

fn show_card_grid(ui: &mut egui::Ui, input: CardGridInput<'_>, state: &mut AppState) {
    let mut clicked: Option<CardGridAction> = None;
    let muted = theme::current(ui).muted;
    let available_w = ui.available_width().max(media_card::MIN_CARD_W);
    let available_h = ui
        .available_height()
        .min(if input.height > 0.0 {
            input.height
        } else {
            f32::MAX
        })
        .max(media_card::MIN_CARD_H);
    let metrics = media_card::grid_metrics(available_w, input.clips.len());

    egui::ScrollArea::vertical()
        .id_salt("ingest_media_grid")
        .auto_shrink([false, false])
        .max_height(available_h)
        .show_viewport(ui, |ui, viewport| {
            ui.set_min_width(available_w);
            if input.clips.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.colored_label(muted, input.empty_message);
                });
                return;
            }

            let total_rows = input.clips.len().div_ceil(metrics.cols);
            let row_stride = metrics.card_h + metrics.gap;
            let first_row = (viewport.top() / row_stride).floor().max(0.0) as usize;
            let last_row = ((viewport.bottom() / row_stride).ceil() as usize + 1).min(total_rows);

            ui.add_space(first_row as f32 * row_stride);
            for row_idx in first_row..last_row {
                let start = row_idx * metrics.cols;
                let end = (start + metrics.cols).min(input.clips.len());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = metrics.gap;
                    for clip in &input.clips[start..end] {
                        let focused = clip.id == input.selected_id;
                        let thumb = input.poster_textures.get(&clip.id);
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(metrics.card_w, metrics.card_h),
                            egui::Sense::click(),
                        );
                        media_card::paint_media_card(
                            ui,
                            rect,
                            &MediaCardInput {
                                title: &clip.name,
                                duration_sec: clip.duration_sec,
                                duration_label: "",
                                import_status: &clip.ingest_status,
                                status_proxy: "pending",
                                status_original: "pending",
                                focused,
                                checked: clip.selected,
                                features: MediaCardFeatures::INGEST,
                                thumb,
                                tc: &duration_label,
                            },
                        );
                        if response.clicked() {
                            let checkbox_click =
                                response.interact_pointer_pos().is_some_and(|pos| {
                                    media_card::selection_check_hit_rect(rect).contains(pos)
                                });
                            clicked = Some(if checkbox_click {
                                CardGridAction::ToggleSelection(clip.id.clone())
                            } else {
                                CardGridAction::Activate(clip.id.clone())
                            });
                        }
                    }
                });
                ui.add_space(metrics.gap);
            }

            let rendered_rows = last_row.saturating_sub(first_row);
            let remaining_rows = total_rows.saturating_sub(first_row + rendered_rows);
            ui.add_space(remaining_rows as f32 * row_stride);
        });

    if let Some(action) = clicked {
        match action {
            CardGridAction::Activate(id) => {
                if let Some(index) = state.clips.iter().position(|clip| clip.id == id) {
                    state.focus_clip(index);
                }
            }
            CardGridAction::ToggleSelection(id) => state.toggle_clip_selection(&id),
        }
    }
}

fn source_dock_panel(
    ui: &mut egui::Ui,
    state: &AppState,
    film_frames: &[FilmFrame],
) -> SourceDockAction {
    let clip = state.active_clip();
    let duration_sec = clip.map(|clip| clip.duration_sec).unwrap_or(0.0);
    let fps = 25.0;
    let fps_confirmed = duration_sec.is_finite() && duration_sec > 0.0;
    let duration_frames = if fps_confirmed {
        (duration_sec * fps).round().max(1.0) as i64
    } else {
        1
    };
    let tc_frame = |frame: i64| {
        if fps_confirmed {
            format_tc(frame_to_seconds(frame.max(0), fps), fps)
        } else {
            "--:--:--:--".into()
        }
    };
    let timeline_model = TimelineProgressModel::from_ranges(
        0.0,
        duration_frames,
        0,
        0,
        duration_frames,
        0,
        duration_frames,
    );
    let label = clip
        .map(|clip| clip.name.as_str())
        .or_else(|| {
            state
                .identified_source
                .as_ref()
                .map(|source| source.display_name.as_str())
        })
        .unwrap_or("—");
    let selected_n = state.selected_clip_count();
    let total = state.clips.len();
    let imported = state.imported_clip_count();
    let status = if state.probe_busy {
        format!("probe u tijeku · {selected_n}/{total}")
    } else if state.ingest_busy {
        format!("ingest u tijeku · {selected_n}/{total}")
    } else {
        format!("{imported} uvezeno · {selected_n}/{total}")
    };
    let empty_peaks: &[f32] = &[];

    source_dock::show(
        ui,
        SourceDockInput {
            clip_label: label,
            source_in_frame: 0,
            source_out_frame: duration_frames,
            timeline_model,
            focus: TimelineFocusPaint::Playhead,
            a1_peaks: empty_peaks,
            a2_peaks: empty_peaks,
            frames: film_frames,
            tc_frame: &tc_frame,
            show_header: true,
            show_edit_actions: false,
            show_import_actions: true,
            archive_original: state.archive_original,
            archive_original_available: true,
            ai_mining: state.ai_mining,
            import_enabled: selected_n > 0 && !state.source.busy && !state.probe_busy,
            proxy_poster_approval_count: 0,
            ingest_status: &status,
            expanded_audio: state.expanded_audio,
        },
    )
}

fn dispatch_source_dock_action(state: &mut AppState, action: SourceDockAction) -> ShellCommand {
    match action {
        SourceDockAction::None => ShellCommand::None,
        SourceDockAction::CueFrame(frame) => {
            state.status_line = format!("Frame: {frame}");
            ShellCommand::None
        }
        SourceDockAction::ToggleAudioExpand(lane) => {
            state.expanded_audio = state.expanded_audio.toggle(lane);
            ShellCommand::None
        }
        SourceDockAction::Reload => ShellCommand::ScanSelectedSource,
        SourceDockAction::ImportSelected => ShellCommand::ProbeSelectedClips,
        SourceDockAction::SelectAll => {
            state.select_all_clips();
            ShellCommand::None
        }
        SourceDockAction::ClearSelection => {
            state.clear_clip_selection();
            ShellCommand::None
        }
        SourceDockAction::SetArchive(value) => {
            state.archive_original = value;
            ShellCommand::None
        }
        SourceDockAction::SetAiMining(value) => {
            state.ai_mining = value;
            ShellCommand::None
        }
        SourceDockAction::ApproveProxyPosters
        | SourceDockAction::SaveVirtualShot
        | SourceDockAction::CreatePart(_)
        | SourceDockAction::CreateCover => ShellCommand::None,
    }
}

fn set_command(command: &mut ShellCommand, next: ShellCommand) {
    if *command == ShellCommand::None {
        *command = next;
    }
}

fn frame_to_seconds(frame: i64, fps: f64) -> f64 {
    if !fps.is_finite() || fps <= 0.0 {
        0.0
    } else {
        frame as f64 / fps
    }
}

fn format_tc(sec: f64, fps: f64) -> String {
    if !fps.is_finite() || fps <= 0.0 {
        return "--:--:--:--".into();
    }
    let sec = if sec.is_finite() && sec > 0.0 {
        sec
    } else {
        0.0
    };
    let total_frames = (sec * fps).round() as i64;
    let fps_i = fps.round().max(1.0) as i64;
    let ff = total_frames.rem_euclid(fps_i);
    let total_sec = total_frames / fps_i;
    let ss = total_sec.rem_euclid(60);
    let total_min = total_sec / 60;
    let mm = total_min.rem_euclid(60);
    let hh = total_min / 60;
    format!("{hh:02}:{mm:02}:{ss:02}:{ff:02}")
}
