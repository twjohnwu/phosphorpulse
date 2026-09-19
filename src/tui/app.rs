//! Application-state shell for the TUI.

use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

use phosphorpulse::{atomic_write::write_atomic, config::model::Config};
use serde_json::Value;

use phosphorpulse::tui::{i18n::{Key, Lang}, wizard::WizardStep};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScreenId {
    MainMenu,
    RowsSegments,
    SubagentLine,
    ColorsTheme,
    Templates,
    SettingsInstall,
    CodexSettings,
    TmThemeEditor,
    SaveExit,
    Wizard,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Focus {
    List(usize),
    Editing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UiMode {
    Normal,
    SegmentPicker {
        target: SegmentPickerTarget,
        insert_after: bool,
        position: usize,
    },
    TemplateSaveName,
    TemplateLoadName,
    TemplateExportPath,
    TemplateImportPath,
    TemplateImportName,
    CommandName,
    CommandString,
    CommandEdit,
    ConfirmDeleteCommand,
    TemplateSaveOverwrite,
    TemplateImportOverwrite,
    TemplateExportOverwrite,
    TemplateDeleteConfirm,
    SettingsWriteConfirm,
    SettingsBinInstallConfirm,
    WizardBinInstallConfirm,
    CodexThemePicker,
    CodexConfirmWrite,
    TmThemeConfirmWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentPickerTarget {
    Main,
    Subagent,
}

/// Shared UI state. Screens only translate keys into calls on the tested logic
/// modules and update this state.
#[derive(Clone, Debug)]
pub struct AppState {
    pub draft: Config,
    pub focus: Focus,
    pub row_index: usize,
    pub screen: ScreenId,
    pub lang: Lang,
    pub wizard_step: WizardStep,
    /// A rewiring wizard starts from a valid existing draft and may return to
    /// the menu; a first-run wizard retains its TS-compatible quit behavior.
    pub wizard_preserves_existing_config: bool,
    pub error: Option<String>,
    pub status: Option<String>,
    pub input: String,
    pub input_error: Option<Key>,
    pub mode: UiMode,
    pub pending_command: Option<String>,
    pub command_notice: Option<String>,
    pub pending_name: String,
    pub pending_path: PathBuf,
    pub should_quit: bool,
    pub config_dir: PathBuf,
    pub codex_config: Option<phosphorpulse::tui::codex::config_io::ConfigSnapshot>,
    pub codex_edit: Option<phosphorpulse::tui::codex::config_io::CodexConfig>,
    pub tmtheme: Option<phosphorpulse::tui::codex::tmtheme::TmThemeSnapshot>,
    pub tmtheme_original: Option<phosphorpulse::tui::codex::tmtheme::TmThemeSnapshot>,
    pub tmtheme_split: bool,
}

impl AppState {
    pub fn new(draft: Config, screen: ScreenId) -> Self {
        let config_dir = phosphorpulse::config::settings_path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let lang = match draft.0.get("language").and_then(Value::as_str) {
            Some("zh-TW") => Lang::ZhTw,
            _ => Lang::En,
        };
        Self {
            draft,
            focus: Focus::List(0),
            row_index: 0,
            screen,
            lang,
            wizard_step: WizardStep::Language,
            wizard_preserves_existing_config: false,
            error: None,
            status: None,
            input: String::new(),
            input_error: None,
            should_quit: false,
            config_dir,
            mode: UiMode::Normal,
            pending_command: None,
            command_notice: None,
            pending_name: String::new(),
            pending_path: PathBuf::new(),
            codex_config: None,
            codex_edit: None,
            tmtheme: None,
            tmtheme_original: None,
            tmtheme_split: false,
        }
    }

    pub fn selected(&self) -> usize {
        match self.focus {
            Focus::List(index) => index,
            Focus::Editing => 0,
        }
    }
    pub fn set_selected(&mut self, index: usize) {
        self.focus = Focus::List(index);
    }
    /// Text inputs share the 256-character cap and grapheme-aware deletion.
    pub fn push_input(&mut self, value: char) {
        if self.input.graphemes(true).count() < 256 {
            self.input.push(value);
        }
    }
    pub fn pop_input_grapheme(&mut self) {
        if let Some((index, _)) = self.input.grapheme_indices(true).last() {
            self.input.truncate(index);
        }
    }
}

use unicode_segmentation::UnicodeSegmentation;

/// Persists the SaveExit draft as this installation's own `settings.json`.
pub fn write_own_config(draft: &Config, config_dir: &Path) -> io::Result<()> {
    // Keep the TUI persistence contract explicit: a first-run HOME has no
    // `.claude/phosphorpulse` directory yet.  `write_atomic` also protects
    // callers generally, but creation belongs to this operation's boundary.
    fs::create_dir_all(config_dir)?;
    let contents = serde_json::to_vec_pretty(draft).map_err(io::Error::other)?;
    write_atomic(&config_dir.join("settings.json"), &contents)
}
