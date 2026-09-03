use serde::{Deserialize, Serialize};

use crate::ui::theme::ThemeId;
use crate::ui::timeline::ExpandedAudio;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FsEntry {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SourceKind {
    #[default]
    Local,
    Lan,
    Intranet,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::Lan => "LAN",
            Self::Intranet => "Intranet",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceBrowserState {
    pub kind: SourceKind,
    pub roots: bool,
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<FsEntry>,
    #[serde(default)]
    pub selected_root_label: Option<String>,
    pub error: Option<String>,
    pub busy: bool,
}

impl SourceBrowserState {
    pub fn initial() -> Self {
        Self {
            kind: SourceKind::Local,
            roots: true,
            path: String::new(),
            parent: None,
            entries: Vec::new(),
            selected_root_label: None,
            error: None,
            busy: false,
        }
    }

    pub fn open_path(&mut self, path: String) {
        self.kind = SourceKind::Local;
        self.roots = path.trim().is_empty();
        self.path = path;
        self.parent = if self.roots {
            None
        } else {
            parent_path(&self.path)
        };
        self.entries.clear();
        self.selected_root_label = None;
        self.error = None;
    }

    pub fn open_location(&mut self, location: String) {
        let trimmed = location.trim();
        if trimmed.starts_with("qnc+lan://") {
            self.open_transport_location(SourceKind::Lan, location);
        } else if trimmed.starts_with("qnc+intranet://") || trimmed.starts_with("qnc://") {
            self.open_transport_location(SourceKind::Intranet, location);
        } else {
            self.open_path(location);
        }
    }

    fn open_transport_location(&mut self, kind: SourceKind, location: String) {
        self.kind = kind;
        self.roots = false;
        self.path = location;
        self.parent = None;
        self.entries.clear();
        self.selected_root_label = None;
        self.error = None;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClipCard {
    pub id: String,
    pub name: String,
    pub duration_sec: f64,
    pub ingest_status: String,
    pub selected: bool,
    pub source_identity: String,
    pub clip_fingerprint: String,
    pub clip_created_at: String,
    pub poster_relative_path: Option<String>,
    pub poster_source: Option<String>,
    #[serde(skip)]
    pub poster_access_path: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum IngestLibraryTab {
    #[default]
    All,
    Virtual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdentifiedSourceSummary {
    pub operator_location: String,
    pub source_identity: String,
    pub source_kind: String,
    pub display_name: String,
    pub transport_uri: String,
    pub content_database_uri: String,
    pub identity_basis: String,
    pub identity_label: String,
    pub identity_value: String,
    pub confidence: String,
    pub seen_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub theme: ThemeId,
    pub source: SourceBrowserState,
    pub identified_source: Option<IdentifiedSourceSummary>,
    pub clips: Vec<ClipCard>,
    pub active_clip_id: Option<String>,
    pub library_tab: IngestLibraryTab,
    pub archive_original: bool,
    pub ai_mining: bool,
    pub ingest_busy: bool,
    pub probe_busy: bool,
    #[serde(skip)]
    pub expanded_audio: ExpandedAudio,
    pub status_line: String,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            theme: ThemeId::Dark,
            source: SourceBrowserState::initial(),
            identified_source: None,
            clips: Vec::new(),
            active_clip_id: None,
            library_tab: IngestLibraryTab::All,
            archive_original: false,
            ai_mining: false,
            ingest_busy: false,
            probe_busy: false,
            expanded_audio: ExpandedAudio::None,
            status_line: "Odaberi izvor za ingest.".into(),
        }
    }

    pub fn active_clip(&self) -> Option<&ClipCard> {
        let id = self.active_clip_id.as_deref()?;
        self.clips.iter().find(|clip| clip.id == id)
    }

    pub fn focus_clip(&mut self, index: usize) {
        if let Some(clip) = self.clips.get(index) {
            self.active_clip_id = Some(clip.id.clone());
            self.status_line = format!("Klip: {}", clip.name);
        }
    }

    pub fn selected_clip_count(&self) -> usize {
        self.clips.iter().filter(|clip| clip.selected).count()
    }

    pub fn selected_clip_ids(&self) -> Vec<String> {
        self.clips
            .iter()
            .filter(|clip| clip.selected)
            .map(|clip| clip.id.clone())
            .collect()
    }

    pub fn replace_clips_preserving_selection(&mut self, clips: Vec<ClipCard>) {
        let selected = self
            .selected_clip_ids()
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let active_id = self.active_clip_id.clone();

        self.clips = clips;
        for clip in &mut self.clips {
            clip.selected = selected.contains(&clip.id);
        }
        self.active_clip_id = active_id
            .filter(|id| self.clips.iter().any(|clip| clip.id == *id))
            .or_else(|| self.clips.first().map(|clip| clip.id.clone()));
    }

    pub fn imported_clip_count(&self) -> usize {
        self.clips
            .iter()
            .filter(|clip| {
                matches!(
                    clip.ingest_status.trim().to_ascii_lowercase().as_str(),
                    "imported" | "done"
                )
            })
            .count()
    }

    pub fn select_all_clips(&mut self) {
        for clip in &mut self.clips {
            clip.selected = true;
        }
        self.status_line = format!(
            "Odabrano: {}/{}",
            self.selected_clip_count(),
            self.clips.len()
        );
    }

    pub fn clear_clip_selection(&mut self) {
        for clip in &mut self.clips {
            clip.selected = false;
        }
        self.status_line = "Odabir očišćen.".into();
    }

    pub fn toggle_clip_selection(&mut self, clip_id: &str) {
        if let Some(clip) = self.clips.iter_mut().find(|clip| clip.id == clip_id) {
            clip.selected = !clip.selected;
            self.status_line = format!(
                "Odabrano: {}/{}",
                self.selected_clip_count(),
                self.clips.len()
            );
        }
    }
}

pub fn duration_label(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "—".into();
    }
    let total = seconds.round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn parent_path(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches(['\\', '/']);
    if trimmed.len() <= 3 && trimmed.as_bytes().get(1) == Some(&b':') {
        return Some(String::new());
    }
    let index = trimmed.rfind(['\\', '/'])?;
    if index <= 2 && trimmed.as_bytes().get(1) == Some(&b':') {
        Some(format!("{}\\", &trimmed[..2]))
    } else {
        Some(trimmed[..index].to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_label_uses_compact_timecode() {
        assert_eq!(duration_label(0.0), "—");
        assert_eq!(duration_label(8.2), "0:08");
        assert_eq!(duration_label(65.0), "1:05");
        assert_eq!(duration_label(3661.0), "1:01:01");
    }

    #[test]
    fn initial_state_has_no_seed_sources_or_clips() {
        let state = AppState::new();
        assert!(state.clips.is_empty());
        assert!(state.active_clip_id.is_none());
        assert!(state.identified_source.is_none());
        assert!(state.source.entries.is_empty());
        assert!(state.status_line.contains("izvor"));
    }

    #[test]
    fn source_browser_initial_location_keeps_transport_kind() {
        let mut lan = SourceBrowserState::initial();
        lan.open_location("qnc+lan://nas-qnc/cards/A001".into());
        assert_eq!(lan.kind, SourceKind::Lan);
        assert_eq!(lan.path, "qnc+lan://nas-qnc/cards/A001");
        assert!(!lan.roots);

        let mut intranet = SourceBrowserState::initial();
        intranet.open_location("qnc+intranet://gateway/cards/A001".into());
        assert_eq!(intranet.kind, SourceKind::Intranet);
        assert_eq!(intranet.path, "qnc+intranet://gateway/cards/A001");
        assert!(!intranet.roots);
    }
}
