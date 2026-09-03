use eframe::egui::{self, RichText, Vec2};

use crate::{
    model::{FsEntry, SourceBrowserState, SourceKind},
    ui::theme::{self, MUTED, TEXT},
};

const UP_COL_W: f32 = 42.0;
const DISKS_COL_W: f32 = 58.0;
const NAV_GAP_W: f32 = 12.0;
const BACK_LABEL: &str = "Gore";

pub enum LocationBrowserAction {
    None,
    SelectKind(SourceKind),
    OpenPath(String),
    Confirm,
    Cancel,
}

pub fn show(
    ui: &mut egui::Ui,
    state: &SourceBrowserState,
    source_serial: Option<&str>,
) -> LocationBrowserAction {
    let mut action = LocationBrowserAction::None;

    ui.horizontal(|ui| {
        for kind in [SourceKind::Local, SourceKind::Lan, SourceKind::Intranet] {
            let selected = state.kind == kind;
            if theme::link_tab(ui, kind.label(), selected).clicked() && !selected {
                action = LocationBrowserAction::SelectKind(kind);
            }
            ui.add_space(10.0);
        }
    });

    ui.add_space(10.0);
    if matches!(state.kind, SourceKind::Local) && state.roots {
        show_root_disk_table(ui, state, &mut action);
    } else if matches!(state.kind, SourceKind::Lan | SourceKind::Intranet) {
        show_transport_table(ui, state, &mut action);
    } else {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let can_up =
                matches!(state.kind, SourceKind::Local) && !state.roots && state.parent.is_some();
            if fixed_text_link(ui, BACK_LABEL, can_up, UP_COL_W).clicked() {
                action = LocationBrowserAction::OpenPath(state.parent.clone().unwrap_or_default());
            }
            ui.add_space(NAV_GAP_W);
            let can_disks = matches!(state.kind, SourceKind::Local);
            if fixed_text_link(ui, "Disk", can_disks, DISKS_COL_W).clicked() {
                action = LocationBrowserAction::OpenPath(String::new());
            }
            ui.add_space(NAV_GAP_W);
            show_location_breadcrumb(ui, state, source_serial, &mut action);
        });
    }

    if let Some(error) = &state.error {
        ui.add_space(4.0);
        ui.colored_label(egui::Color32::from_rgb(220, 100, 80), error);
    }

    ui.add_space(6.0);
    let tree_h = (ui.available_height() - theme::CHROME_CTRL_H - 8.0).max(40.0);
    egui::ScrollArea::vertical()
        .id_salt("ingest_qnc_location_browser")
        .max_height(tree_h)
        .min_scrolled_height(tree_h)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            match state.kind {
                SourceKind::Local => show_local_tree(ui, state, &mut action),
                SourceKind::Lan | SourceKind::Intranet => {}
            }
        });

    ui.add_space(8.0);
    let can_confirm = !state.busy
        && match state.kind {
            SourceKind::Local => !state.roots && !state.path.trim().is_empty(),
            SourceKind::Lan | SourceKind::Intranet => !state.path.trim().is_empty(),
        };
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), theme::CHROME_CTRL_H),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if theme::action_btn(ui, "Odustani").clicked() {
                action = LocationBrowserAction::Cancel;
            }
            ui.add_enabled_ui(can_confirm, |ui| {
                if theme::primary_btn(ui, "Odaberi").clicked() {
                    action = LocationBrowserAction::Confirm;
                }
            });
        },
    );

    action
}

fn fixed_text_link(ui: &mut egui::Ui, label: &str, enabled: bool, width: f32) -> egui::Response {
    ui.allocate_ui_with_layout(
        Vec2::new(width, theme::CHROME_CTRL_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| theme::text_link(ui, label, enabled),
    )
    .inner
}

fn show_local_tree(
    ui: &mut egui::Ui,
    state: &SourceBrowserState,
    action: &mut LocationBrowserAction,
) {
    if state.roots {
        return;
    }

    if state.entries.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(path_tree_offset());
            ui.label(
                RichText::new("Nema podmapa.")
                    .size(theme::FONT_UI)
                    .color(MUTED),
            );
        });
        return;
    }

    for entry in &state.entries {
        if location_tree_row(ui, path_tree_offset(), &entry_display_name(entry, false)) {
            *action = LocationBrowserAction::OpenPath(clean_location_path(&entry.path));
        }
    }
}

fn show_transport_table(
    ui: &mut egui::Ui,
    state: &SourceBrowserState,
    action: &mut LocationBrowserAction,
) {
    egui::Grid::new(format!(
        "ingest_qnc_{}_selector",
        transport_label(state.kind)
    ))
    .num_columns(3)
    .spacing(Vec2::new(10.0, 3.0))
    .show(ui, |ui| {
        grid_label(ui, BACK_LABEL, MUTED);
        grid_label(ui, transport_label(state.kind), TEXT);
        grid_label(ui, "", TEXT);
        ui.end_row();

        grid_label(ui, "", MUTED);
        grid_label(ui, "", TEXT);
        let mut path = state.path.clone();
        let hint = match state.kind {
            SourceKind::Lan => "qnc+lan://server/share",
            SourceKind::Intranet => "qnc+intranet://gateway/source",
            SourceKind::Local => "",
        };
        let response = ui.add_sized(
            Vec2::new(ui.available_width().max(80.0), theme::CHROME_CTRL_H),
            egui::TextEdit::singleline(&mut path)
                .hint_text(hint)
                .font(egui::TextStyle::Body),
        );
        if response.changed() {
            *action = LocationBrowserAction::OpenPath(path);
        }
        ui.end_row();
    });
}

fn transport_label(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Lan => "LAN:",
        SourceKind::Intranet => "Intranet:",
        SourceKind::Local => "Disk:",
    }
}

fn path_tree_offset() -> f32 {
    UP_COL_W + NAV_GAP_W + DISKS_COL_W + NAV_GAP_W
}

fn location_tree_row(ui: &mut egui::Ui, offset: f32, label: &str) -> bool {
    ui.horizontal(|ui| {
        ui.add_space(offset);
        theme::text_link(ui, label, true).clicked()
    })
    .inner
}

fn entry_display_name(entry: &FsEntry, roots: bool) -> String {
    if !entry.name.trim().is_empty() {
        return clean_location_path(&entry.name);
    }
    if roots {
        clean_location_path(&entry.path)
    } else {
        path_leaf(&entry.path)
    }
}

fn path_leaf(path: &str) -> String {
    let clean = clean_location_path(path);
    let trimmed = clean.trim_end_matches(['\\', '/']);
    trimmed
        .rsplit(['\\', '/'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

pub fn clean_location_path(path: &str) -> String {
    path.trim().to_string()
}

fn location_label(state: &SourceBrowserState) -> String {
    match state.kind {
        SourceKind::Local if state.roots => "Diskovi".into(),
        SourceKind::Local if !state.path.trim().is_empty() => short_path(&state.path),
        SourceKind::Lan => "LAN".into(),
        SourceKind::Intranet => "Intranet".into(),
        _ => "-".into(),
    }
}

fn show_location_breadcrumb(
    ui: &mut egui::Ui,
    state: &SourceBrowserState,
    source_serial: Option<&str>,
    action: &mut LocationBrowserAction,
) {
    if matches!(state.kind, SourceKind::Local) && state.roots {
        return;
    }

    if !matches!(state.kind, SourceKind::Local) {
        ui.label(
            RichText::new(location_label(state))
                .size(theme::FONT_UI)
                .color(TEXT),
        );
        show_source_serial(ui, source_serial);
        return;
    }

    let parts = breadcrumb_parts(&state.path);
    if parts.is_empty() {
        ui.label(
            RichText::new(short_path(&state.path))
                .size(theme::FONT_UI)
                .color(TEXT),
        );
        show_source_serial(ui, source_serial);
        return;
    }

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (index, (label, path)) in parts.iter().enumerate() {
            if index > 0 {
                ui.label(RichText::new("\\").size(theme::FONT_UI).color(MUTED));
            }
            if theme::text_link(ui, label, true).clicked() {
                *action = LocationBrowserAction::OpenPath(path.clone());
            }
        }
        show_source_serial(ui, source_serial);
    });
}

fn show_source_serial(ui: &mut egui::Ui, source_serial: Option<&str>) {
    let Some(source_serial) = source_serial
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    ui.add_space(10.0);
    ui.label(
        RichText::new(source_serial)
            .size(theme::FONT_UI)
            .color(MUTED),
    );
}

fn show_root_disk_table(
    ui: &mut egui::Ui,
    state: &SourceBrowserState,
    action: &mut LocationBrowserAction,
) {
    if state.entries.is_empty() {
        egui::Grid::new("ingest_qnc_root_disk_table_empty")
            .num_columns(5)
            .spacing(Vec2::new(10.0, 3.0))
            .show(ui, |ui| {
                grid_label(ui, BACK_LABEL, MUTED);
                grid_label(ui, "Disk:", TEXT);
                grid_label(ui, "", TEXT);
                grid_label(ui, "", TEXT);
                grid_label(ui, "", TEXT);
                ui.end_row();

                grid_label(ui, "", MUTED);
                grid_label(ui, "", TEXT);
                grid_label(ui, "Nema diskova.", MUTED);
                grid_label(ui, "", TEXT);
                grid_label(ui, "", TEXT);
                ui.end_row();
            });
        return;
    }

    egui::Grid::new("ingest_qnc_root_disk_table")
        .num_columns(5)
        .spacing(Vec2::new(10.0, 3.0))
        .show(ui, |ui| {
            grid_label(ui, BACK_LABEL, MUTED);
            grid_label(ui, "Disk:", TEXT);
            grid_label(ui, "", TEXT);
            grid_label(ui, "", TEXT);
            grid_label(ui, "", TEXT);
            ui.end_row();

            for entry in &state.entries {
                let columns = root_disk_columns(entry);
                grid_label(ui, "", MUTED);
                grid_label(ui, "", TEXT);
                let mut clicked = false;
                clicked |= grid_link(ui, &columns.drive).clicked();
                clicked |= grid_link(ui, &columns.serial).clicked();
                clicked |= grid_link(ui, &columns.name).clicked();
                if clicked {
                    *action = LocationBrowserAction::OpenPath(clean_location_path(&entry.path));
                }
                ui.end_row();
            }
        });
}

fn root_disk_label(entry: &FsEntry) -> String {
    let path = clean_location_path(&entry.path);
    let name = clean_location_path(&entry.name);
    if name.is_empty() {
        return path;
    }
    name
}

struct RootDiskColumns {
    drive: String,
    serial: String,
    name: String,
}

fn root_disk_columns(entry: &FsEntry) -> RootDiskColumns {
    let path = clean_location_path(&entry.path);
    let display = root_disk_label(entry);
    let drive = if is_windows_drive_rooted(&path) {
        path[..2].to_owned()
    } else if path == "/" {
        "/".into()
    } else if !display.trim().is_empty() {
        display
            .split_whitespace()
            .next()
            .unwrap_or(display.trim())
            .to_owned()
    } else {
        short_path(&path)
    };

    let rest = display
        .strip_prefix(&drive)
        .unwrap_or(display.as_str())
        .trim();
    let mut parts = rest.split_whitespace();
    let serial = parts.next().unwrap_or_default().to_owned();
    let name = parts.collect::<Vec<_>>().join(" ");

    RootDiskColumns {
        drive,
        serial,
        name,
    }
}

fn grid_label(ui: &mut egui::Ui, label: &str, color: egui::Color32) {
    ui.label(RichText::new(label).size(theme::FONT_UI).color(color));
}

fn grid_link(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add_enabled(
        !label.trim().is_empty(),
        egui::Label::new(RichText::new(label).size(theme::FONT_UI).color(TEXT))
            .sense(egui::Sense::click())
            .selectable(false),
    )
}

fn breadcrumb_parts(path: &str) -> Vec<(String, String)> {
    let clean = clean_location_path(path);
    if clean.is_empty() {
        return Vec::new();
    }

    if is_qnc_transport_path(&clean) {
        return qnc_uri_breadcrumb_parts(&clean);
    }

    if is_windows_drive_rooted(&clean) {
        let drive = clean[..2].to_string();
        let mut out = vec![(drive.clone(), format!("{drive}\\"))];
        let rest = clean[3..].trim_matches(['\\', '/']);
        let mut current = format!("{drive}\\");
        for part in rest.split(['\\', '/']).filter(|part| !part.is_empty()) {
            if !current.ends_with('\\') {
                current.push('\\');
            }
            current.push_str(part);
            out.push((part.to_string(), current.clone()));
        }
        return out;
    }

    if clean.starts_with("\\\\") {
        let mut out = Vec::new();
        let mut current = String::from("\\\\");
        for part in clean
            .trim_start_matches('\\')
            .split('\\')
            .filter(|part| !part.is_empty())
        {
            if current != "\\\\" {
                current.push('\\');
            }
            current.push_str(part);
            out.push((part.to_string(), current.clone()));
        }
        return out;
    }

    if clean.starts_with('/') {
        let mut out = vec![("/".to_string(), "/".to_string())];
        let mut current = String::from("/");
        for part in clean
            .trim_start_matches('/')
            .split('/')
            .filter(|part| !part.is_empty())
        {
            if !current.ends_with('/') {
                current.push('/');
            }
            current.push_str(part);
            out.push((part.to_string(), current.clone()));
        }
        return out;
    }

    let mut out = Vec::new();
    let mut current = String::new();
    for part in clean.split(['\\', '/']).filter(|part| !part.is_empty()) {
        if !current.is_empty() {
            current.push('\\');
        }
        current.push_str(part);
        out.push((part.to_string(), current.clone()));
    }
    out
}

fn is_qnc_transport_path(path: &str) -> bool {
    path.starts_with("qnc+local://")
        || path.starts_with("qnc+lan://")
        || path.starts_with("qnc+intranet://")
        || path.starts_with("qnc://")
}

fn qnc_uri_breadcrumb_parts(uri: &str) -> Vec<(String, String)> {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return Vec::new();
    };

    let mut out = vec![(format!("{scheme}://"), format!("{scheme}://"))];
    let mut current = format!("{scheme}://");
    for part in rest.split('/').filter(|part| !part.is_empty()) {
        if current.ends_with("://") {
            current.push_str(part);
        } else {
            current.push('/');
            current.push_str(part);
        }
        out.push((part.to_string(), current.clone()));
    }
    out
}

fn is_windows_drive_rooted(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn short_path(path: &str) -> String {
    let path = clean_location_path(path);
    if path.chars().count() <= 42 {
        return path;
    }
    let tail: String = path
        .chars()
        .rev()
        .take(36)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("...{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_location_path_keeps_qnc_transport_uri() {
        assert_eq!(
            clean_location_path("qnc+local://localhost/card/source-a"),
            "qnc+local://localhost/card/source-a"
        );
    }

    #[test]
    fn breadcrumb_parts_keep_clickable_qnc_transport_chain() {
        let parts = breadcrumb_parts("qnc+local://localhost/card/source-a/DCIM");
        assert_eq!(
            parts,
            vec![
                ("qnc+local://".to_string(), "qnc+local://".to_string()),
                ("localhost".to_string(), "qnc+local://localhost".to_string()),
                ("card".to_string(), "qnc+local://localhost/card".to_string()),
                (
                    "source-a".to_string(),
                    "qnc+local://localhost/card/source-a".to_string()
                ),
                (
                    "DCIM".to_string(),
                    "qnc+local://localhost/card/source-a/DCIM".to_string()
                ),
            ]
        );
    }

    #[test]
    fn root_disk_label_prefers_serial_display_name() {
        let entry = FsEntry {
            name: "G:   de666c9f   MEDIA_CARD".into(),
            path: "G:\\".into(),
        };

        assert_eq!(root_disk_label(&entry), "G:   de666c9f   MEDIA_CARD");
    }

    #[test]
    fn root_disk_columns_split_drive_serial_and_name() {
        let entry = FsEntry {
            name: "C:   z574z57z5   system".into(),
            path: "C:\\".into(),
        };
        let columns = root_disk_columns(&entry);

        assert_eq!(columns.drive, "C:");
        assert_eq!(columns.serial, "z574z57z5");
        assert_eq!(columns.name, "system");
    }

    #[test]
    fn transport_labels_use_same_source_selector_prefixes() {
        assert_eq!(transport_label(SourceKind::Lan), "LAN:");
        assert_eq!(transport_label(SourceKind::Intranet), "Intranet:");
    }
}
