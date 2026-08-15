#[path = "../src/tui/chrome.rs"]
mod chrome;
#[path = "../src/tui/i18n.rs"]
mod i18n;

/// REQ-05 / S-08 — i18n completeness and typed-key gate (invalid keys cannot compile).
#[test]
fn test_s08_i18n_completeness() {
    let expected_ts_keys = [
        "error.file",
        "error.field",
        "error.reason",
        "wizard.language.title",
        "wizard.language.optionEnglish",
        "wizard.language.optionZhTw",
        "wizard.language.prompt",
        "wizard.language.hintChoose",
        "wizard.language.hintConfirm",
        "wizard.nerdFont.detected",
        "wizard.nerdFont.notDetected",
        "wizard.nerdFont.question",
        "wizard.nerdFont.hintYes",
        "wizard.nerdFont.hintNo",
        "wizard.template.confirmPrompt",
        "wizard.template.hintConfirmSelection",
        "binInstall.notFound",
        "binInstall.installCommand",
        "binInstall.rePrompt",
        "binInstall.hintReDetect",
        "binInstall.stillNotFound",
        "binInstall.manualInstall",
        "binInstall.wizardEnded",
        "preview.rendering",
        "settingsInstall.loading",
        "settingsInstall.binInstalled",
        "settingsInstall.binNotInstalled",
        "settingsInstall.statusLineSet",
        "settingsInstall.statusLineUnset",
        "settingsInstall.subagentStatusLineSet",
        "settingsInstall.subagentStatusLineUnset",
        "settingsInstall.hintRewrite",
        "saveExit.saving",
        "saveExit.done",
        "saveExit.error",
        "saveExit.confirmPrompt",
        "saveExit.hintSaveAndExit",
        "settingsWrite.loading",
        "settingsWrite.confirmPrompt",
        "settingsWrite.hintConfirmWrite",
        "settingsWrite.hintCancel",
        "templates.builtinSuffix",
        "templates.loaded",
        "templates.loadFailed",
        "templates.savedAs",
        "templates.exportedTo",
        "templates.importedAs",
        "templates.overwriteConfirm",
        "templates.saveNameLabel",
        "templates.exportPathLabel",
        "templates.importPathLabel",
        "templates.importNameLabel",
        "templates.nameInvalid",
        "templates.hintSave",
        "templates.hintLoad",
        "templates.hintExport",
        "templates.hintImport",
        "templates.hintBackToMenu",
        "colors.depth",
        "colors.hintCycleAdjust",
        "colors.gaugeWidth",
        "colors.gaugeWarnPct",
        "colors.gaugeHotPct",
        "colors.gaugeColorLabel",
        "colors.dirPathDepth",
        "colors.nerdFont",
        "colors.nerdFontOn",
        "colors.nerdFontOff",
        "mainMenu.language",
        "mainMenu.hintMove",
        "mainMenu.hintOpen",
        "mainMenu.hintCycleLanguage",
        "menu.rowsSegments",
        "menu.colorsTheme",
        "menu.templates",
        "menu.subagentLine",
        "menu.settingsInstall",
        "menu.saveExit",
        "rowsSegments.header",
        "rowsSegments.layoutAuto",
        "rowsSegments.layoutFixed",
        "rowsSegments.pomodoroValues",
        "subagentLine.title",
        "common.moveModeSuffix",
        "hints.addA",
        "hints.insertI",
        "hints.deleteD",
        "hints.enterMoveMode",
        "hints.spaceLayout",
        "hints.tabSwitchRow",
        "hints.pomodoroWorkMin",
        "hints.pomodoroRefreshSec",
        "segmentPicker.hintSelect",
        "segmentPicker.hintConfirm",
        "segmentPicker.hintCancel",
        "error.title",
        "error.hintRerunWizard",
        "error.hintFixManually",
    ];
    let keys = i18n::all_keys();

    assert_eq!(
        expected_ts_keys.len(),
        98,
        "frozen TS fixture must retain its 98 keys"
    );
    for expected in expected_ts_keys {
        assert!(
            keys.iter().any(|key| i18n::key_id(key) == expected),
            "missing TS key: {expected}"
        );
    }
    for key in keys {
        assert!(!i18n::t(i18n::Lang::En, key, &[]).is_empty());
        assert!(!i18n::t(i18n::Lang::ZhTw, key, &[]).is_empty());
    }
    let error_file = keys
        .iter()
        .find(|key| i18n::key_id(key) == "error.file")
        .expect("error.file key");
    assert_eq!(
        i18n::t(i18n::Lang::En, error_file, &[("path", "/tmp/config")]),
        "File: /tmp/config"
    );
}

/// REQ-06 / S-10 — Main Menu and Wizard titles include a semantic version.
#[test]
fn test_s10_title_version() {
    for title in [chrome::main_menu_title(), chrome::wizard_title()] {
        let version = title
            .split_once("phosphorpulse v")
            .expect("title must contain phosphorpulse v")
            .1
            .split_whitespace()
            .next()
            .expect("title must contain a version");
        let parts: Vec<_> = version.split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "title version must have three components: {title}"
        );
        assert!(
            parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())),
            "title version must be numeric semver: {title}"
        );
    }
}
