//! Setup wizard shell for the TUI.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    Language,
    NerdFont,
    BinInstall,
    BinStillMissing,
    Template,
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardInput {
    ConfirmLanguage,
    ConfirmNerdFont { bin_found: bool },
    RedetectBin { bin_found: bool },
    ConfirmTemplate,
    DeclineSettingsWrite,
    ConfirmSettingsWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardAction {
    WriteSettings,
    WriteOwnConfig,
}

/// Applies one wizard input without performing terminal or filesystem I/O.
///
/// Inputs that do not apply to the current screen are ignored, matching the
/// frozen Ink screens' key handlers.
pub fn transition(step: WizardStep, input: WizardInput) -> (WizardStep, Vec<WizardAction>) {
    use WizardInput::*;
    use WizardStep::*;

    match (step, input) {
        (Language, ConfirmLanguage) => (NerdFont, vec![]),
        (NerdFont, ConfirmNerdFont { bin_found: true }) => (Template, vec![]),
        (NerdFont, ConfirmNerdFont { bin_found: false }) => (BinInstall, vec![]),
        (BinInstall, RedetectBin { bin_found: true }) => (Template, vec![]),
        (BinInstall, RedetectBin { bin_found: false }) => (BinStillMissing, vec![]),
        (BinStillMissing, RedetectBin { bin_found: true }) => (Template, vec![]),
        (Template, ConfirmTemplate) => (Confirm, vec![]),
        (Confirm, DeclineSettingsWrite) => (Template, vec![]),
        (Confirm, ConfirmSettingsWrite) => (
            Confirm,
            vec![WizardAction::WriteSettings, WizardAction::WriteOwnConfig],
        ),
        (step, _) => (step, vec![]),
    }
}
