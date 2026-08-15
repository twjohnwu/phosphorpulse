//! RED coverage for the pure-logic TUI setup wizard.

#[path = "../src/tui/wizard.rs"]
mod wizard;

use wizard::{WizardAction, WizardInput, WizardStep, transition};

/// REQ-02 / S-09: wizard transitions match the frozen TypeScript six-state
/// flow, including the missing-bin dead end and ordered confirmed writes.
#[test]
fn test_s09_wizard_transitions() {
    let language = WizardStep::Language;
    let nerd_font = WizardStep::NerdFont;
    let template = WizardStep::Template;
    let confirm = WizardStep::Confirm;

    assert_eq!(
        transition(language, WizardInput::ConfirmLanguage),
        (nerd_font, vec![]),
    );
    assert_eq!(
        transition(nerd_font, WizardInput::ConfirmNerdFont { bin_found: true }),
        (template, vec![]),
    );
    assert_eq!(
        transition(template, WizardInput::ConfirmTemplate),
        (confirm, vec![]),
    );

    assert_eq!(
        transition(nerd_font, WizardInput::ConfirmNerdFont { bin_found: false }),
        (WizardStep::BinInstall, vec![]),
    );
    assert_eq!(
        transition(
            WizardStep::BinInstall,
            WizardInput::RedetectBin { bin_found: true }
        ),
        (template, vec![]),
    );
    assert_eq!(
        transition(
            WizardStep::BinInstall,
            WizardInput::RedetectBin { bin_found: false }
        ),
        (WizardStep::BinStillMissing, vec![]),
    );
    assert_eq!(
        transition(WizardStep::BinStillMissing, WizardInput::ConfirmLanguage),
        (WizardStep::BinStillMissing, vec![]),
        "non-redetect inputs remain ignored in binStillMissing",
    );
    assert_eq!(
        transition(
            WizardStep::BinStillMissing,
            WizardInput::RedetectBin { bin_found: false }
        ),
        (WizardStep::BinStillMissing, vec![]),
        "a failed re-detect remains ignored in binStillMissing",
    );
    assert_eq!(
        transition(
            WizardStep::BinStillMissing,
            WizardInput::RedetectBin { bin_found: true }
        ),
        (template, vec![]),
        "a successful re-detect is the sole binStillMissing escape",
    );

    assert_eq!(
        transition(confirm, WizardInput::DeclineSettingsWrite),
        (template, vec![]),
    );
    assert_eq!(
        transition(confirm, WizardInput::ConfirmSettingsWrite),
        (
            confirm,
            vec![WizardAction::WriteSettings, WizardAction::WriteOwnConfig],
        ),
    );
}
