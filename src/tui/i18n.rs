//! Internationalization strings for the TUI.

/// A display language supported by the TUI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lang {
    En,
    ZhTw,
}

macro_rules! message_table {
    ($($variant:ident => ($id:literal, $en:literal, $zh_tw:literal),)+) => {
        /// A typed TUI message key.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Key { $($variant,)+ }

        const KEYS: &[Key] = &[$(Key::$variant,)+];

        fn message(key: Key, lang: Lang) -> &'static str {
            match (key, lang) {
                $((Key::$variant, Lang::En) => $en,
                  (Key::$variant, Lang::ZhTw) => $zh_tw,)+
            }
        }

        /// Returns the frozen TypeScript identifier corresponding to a key.
        pub fn key_id(key: &Key) -> &'static str {
            match key { $(Key::$variant => $id,)+ }
        }
    };
}

message_table! {
    ErrorFile => ("error.file", "File: {path}", "檔案: {path}"),
    ErrorField => ("error.field", "Field: {path}", "欄位: {path}"),
    ErrorReason => ("error.reason", "Reason: {reason}", "原因: {reason}"),
    WizardLanguageTitle => ("wizard.language.title", "Language / 語言", "Language / 語言"),
    WizardLanguageOptionEnglish => ("wizard.language.optionEnglish", "English", "English"),
    WizardLanguageOptionZhTw => ("wizard.language.optionZhTw", "繁體中文", "繁體中文"),
    WizardLanguagePrompt => ("wizard.language.prompt", "↑/↓ choose, Enter confirms", "↑/↓ 選擇，Enter 確認"),
    WizardLanguageHintChoose => ("wizard.language.hintChoose", "[↑/↓] choose", "[↑/↓] 選擇"),
    WizardLanguageHintConfirm => ("wizard.language.hintConfirm", "[Enter] confirm", "[Enter] 確認"),
    WizardNerdFontDetected => ("wizard.nerdFont.detected", "Nerd Font detected, will use Powerline separators", "偵測到 Nerd Font，將使用 Powerline 分隔符"),
    WizardNerdFontNotDetected => ("wizard.nerdFont.notDetected", "No Nerd Font detected, switched to ASCII mode", "未偵測到 Nerd Font，已切換 ASCII 模式"),
    WizardNerdFontQuestion => ("wizard.nerdFont.question", "Does your terminal have a Nerd Font? [{yn}]", "終端機有 Nerd Font? [{yn}]"),
    WizardNerdFontHintYes => ("wizard.nerdFont.hintYes", "[y] yes", "[y] 是"),
    WizardNerdFontHintNo => ("wizard.nerdFont.hintNo", "[n] no", "[n] 否"),
    WizardTemplateConfirmPrompt => ("wizard.template.confirmPrompt", "Press Enter to confirm selection and preview the settings.json changes", "Enter 確認選擇並預覽 settings.json 變更"),
    WizardTemplateHintConfirmSelection => ("wizard.template.hintConfirmSelection", "[Enter] confirm selection", "[Enter] 確認選擇"),
    BinInstallNotFound => ("binInstall.notFound", "Global phosphorpulse command not detected", "未偵測到全域 phosphorpulse 指令"),
    BinInstallInstallCommand => ("binInstall.installCommand", "Install command: {cmd}", "安裝指令: {cmd}"),
    BinInstallRePrompt => ("binInstall.rePrompt", "Press any key to re-detect after installing", "安裝完成後按任意鍵重新偵測"),
    BinInstallHintReDetect => ("binInstall.hintReDetect", "[any key] re-detect", "[任意鍵] 重新偵測"),
    BinInstallHintInstallBin => ("binInstall.hintInstallBin", "[b] install to ~/.local/bin", "[b] 安裝到 ~/.local/bin"),
    BinInstallStillNotFound => ("binInstall.stillNotFound", "Still no global phosphorpulse command detected", "仍未偵測到全域 phosphorpulse 指令"),
    BinInstallManualInstall => ("binInstall.manualInstall", "To install manually, copy the binary to ~/.local/bin, or press [b].", "請將執行檔複製到 ~/.local/bin，或按 [b] 安裝。"),
    BinInstallInstalledTo => ("binInstall.installedTo", "installed to {path}", "已安裝至 {path}"),
    BinInstallWizardEnded => ("binInstall.wizardEnded", "First-run wizard ended, settings.json was not written", "已結束首跑精靈，未寫入 settings.json"),
    PreviewRendering => ("preview.rendering", "preview: rendering…", "preview: 渲染中…"),
    SettingsInstallLoading => ("settingsInstall.loading", "Loading install status…", "讀取安裝狀態…"),
    SettingsInstallBinInstalled => ("settingsInstall.binInstalled", "Global bin: installed ({path})", "全域 bin: 已安裝 ({path})"),
    SettingsInstallBinNotInstalled => ("settingsInstall.binNotInstalled", "Global bin: not installed (install command: {cmd})", "全域 bin: 未安裝 (安裝指令: {cmd})"),
    SettingsInstallStatusLineSet => ("settingsInstall.statusLineSet", "statusLine: configured ({cmd})", "statusLine: 已設定 ({cmd})"),
    SettingsInstallStatusLineUnset => ("settingsInstall.statusLineUnset", "statusLine: not configured", "statusLine: 未設定"),
    SettingsInstallSubagentStatusLineSet => ("settingsInstall.subagentStatusLineSet", "subagentStatusLine: configured ({cmd})", "subagentStatusLine: 已設定 ({cmd})"),
    SettingsInstallSubagentStatusLineUnset => ("settingsInstall.subagentStatusLineUnset", "subagentStatusLine: not configured", "subagentStatusLine: 未設定"),
    SettingsInstallHintRewrite => ("settingsInstall.hintRewrite", "[w] rewrite settings.json", "[w] 重寫 settings.json"),
    SettingsInstallHintInstallBin => ("settingsInstall.hintInstallBin", "[b] install global bin", "[b] 安裝全域 bin"),
    SettingsInstallHintReinstallBin => ("settingsInstall.hintReinstallBin", "[b] reinstall/upgrade bin", "[b] 重新安裝/升級 bin"),
    SettingsInstallBinInstallConfirm => ("settingsInstall.binInstallConfirm", "Install current executable to {path}? [y/n]", "將目前執行檔安裝到 {path}？[y/n]"),
    SettingsInstallBinOverwriteConfirm => ("settingsInstall.binOverwriteConfirm", "{path} already exists; overwrite with current executable? [y/n]", "{path} 已存在；要以目前執行檔覆寫嗎？[y/n]"),
    SettingsInstallBinPathHint => ("settingsInstall.binPathHint", "Add ~/.local/bin to PATH and restart your shell to run phosphorpulse by name.", "請將 ~/.local/bin 加入 PATH，並重新啟動 shell，才能以名稱執行 phosphorpulse。"),
    SettingsInstallBinInstallFailed => ("settingsInstall.binInstallFailed", "Could not install {path}: {reason}", "無法安裝 {path}：{reason}"),
    SettingsInstallBinHomeMissing => ("settingsInstall.binHomeMissing", "Could not install ~/.local/bin/phosphorpulse: HOME is not set", "無法安裝 ~/.local/bin/phosphorpulse：未設定 HOME"),
    SettingsInstallCodexConfigured => ("settingsInstall.codexConfigured", "Codex: configured ({items} items, colors {colors}, theme {theme})", "Codex：已設定（{items} 個項目，色彩{colors}，主題 {theme}）"),
    SettingsInstallCodexNotConfigured => ("settingsInstall.codexNotConfigured", "Codex: not configured", "Codex：未設定"),
    SettingsInstallCodexColorsOn => ("settingsInstall.codexColorsOn", "on", "開啟"),
    SettingsInstallCodexColorsOff => ("settingsInstall.codexColorsOff", "off", "關閉"),
    SaveExitSaving => ("saveExit.saving", "Saving…", "儲存中…"),
    SaveExitDone => ("saveExit.done", "Saved and exited", "已儲存並結束"),
    SaveExitError => ("saveExit.error", "Save failed: {message}", "儲存失敗: {message}"),
    SaveExitConfirmPrompt => ("saveExit.confirmPrompt", "Save settings and exit? [y/Enter]", "儲存設定並離開？ [y/Enter]"),
    SaveExitHintSaveAndExit => ("saveExit.hintSaveAndExit", "[y/Enter] save and exit", "[y/Enter] 儲存並離開"),
    SettingsWriteLoading => ("settingsWrite.loading", "Loading existing settings.json…", "讀取現有 settings.json…"),
    SettingsWriteConfirmPrompt => ("settingsWrite.confirmPrompt", "Confirm write? [y/n]", "確認寫入？ [y/n]"),
    SettingsWriteHintConfirmWrite => ("settingsWrite.hintConfirmWrite", "[y] confirm write", "[y] 確認寫入"),
    SettingsWriteHintCancel => ("settingsWrite.hintCancel", "[n] cancel", "[n] 取消"),
    TemplatesBuiltinSuffix => ("templates.builtinSuffix", " (built-in, read-only)", " (內建, 唯讀)"),
    TemplatesLoaded => ("templates.loaded", "Loaded {name}", "已載入 {name}"),
    TemplatesLoadFailed => ("templates.loadFailed", "Load failed: {message}", "載入失敗: {message}"),
    TemplatesSavedAs => ("templates.savedAs", "Saved as {name}", "已存為 {name}"),
    TemplatesExportedTo => ("templates.exportedTo", "Exported to {path}", "已匯出至 {path}"),
    TemplatesImportedAs => ("templates.importedAs", "Imported as {name}", "已匯入為 {name}"),
    TemplatesDeleted => ("templates.deleted", "Deleted {name}", "已刪除 {name}"),
    TemplatesOverwriteConfirm => ("templates.overwriteConfirm", "{path} already exists, overwrite? [y/n]", "{path} 已存在，覆寫？ [y/n]"),
    TemplatesDeleteConfirm => ("templates.deleteConfirm", "Delete {name}? [y/n]", "要刪除 {name} 嗎？ [y/n]"),
    TemplatesSaveNameLabel => ("templates.saveNameLabel", "Save as new template name", "另存新樣板名稱"),
    TemplatesExportPathLabel => ("templates.exportPathLabel", "Export destination path", "匯出目標路徑"),
    TemplatesImportPathLabel => ("templates.importPathLabel", "Import source path", "匯入來源路徑"),
    TemplatesImportNameLabel => ("templates.importNameLabel", "Template name after import", "匯入後樣板名稱"),
    TemplatesNameInvalid => ("templates.nameInvalid", "Name doesn't match {pattern}", "名稱不符 {pattern}"),
    TemplatesHintSave => ("templates.hintSave", "[s] Save as...", "[s] 另存…"),
    TemplatesHintLoad => ("templates.hintLoad", "[l] Load", "[l] 載入"),
    TemplatesHintExport => ("templates.hintExport", "[e] Export", "[e] 匯出"),
    TemplatesHintImport => ("templates.hintImport", "[i] Import", "[i] 匯入"),
    TemplatesHintDelete => ("templates.hintDelete", "[d] Delete", "[d] 刪除"),
    TemplatesHintBackToMenu => ("templates.hintBackToMenu", "[Esc/Enter] back to menu", "[Esc/Enter] 返回選單"),
    ColorsDepth => ("colors.depth", "Color depth: {depth}", "色深: {depth}"),
    ColorsHintCycleAdjust => ("colors.hintCycleAdjust", "[←/→] cycle/adjust", "[←/→] 循環/調整"),
    ColorsHintCmdAddKey => ("colors.hintCmdAddKey", "[a] add", "[a] 新增"),
    ColorsHintCmdDeleteKey => ("colors.hintCmdDeleteKey", "[d] delete", "[d] 刪除"),
    ColorsHintCmdEditKey => ("colors.hintCmdEditKey", "[Enter] edit", "[Enter] 編輯"),
    ColorsGaugeWidth => ("colors.gaugeWidth", "gauge width: {value}", "gauge 寬度: {value}"),
    ColorsGaugeWarnPct => ("colors.gaugeWarnPct", "gauge warn%: {value}", "gauge 警告%: {value}"),
    ColorsGaugeHotPct => ("colors.gaugeHotPct", "gauge hot%: {value}", "gauge 危險%: {value}"),
    ColorsGaugeColorLabel => ("colors.gaugeColorLabel", "gauge {state}  color:", "gauge {state}  顏色:"),
    ColorsDirPathDepth => ("colors.dirPathDepth", "dir path depth: {value}", "dir 路徑深度: {value}"),
    ColorsNerdFont => ("colors.nerdFont", "Nerd Font: {value}", "Nerd Font: {value}"),
    ColorsNerdFontOn => ("colors.nerdFontOn", "on", "開"),
    ColorsNerdFontOff => ("colors.nerdFontOff", "off", "關"),
    ColorsCmdCommand => ("colors.cmdCommand", "command: {value}", "command: {value}"),
    ColorsCmdTimeoutMs => ("colors.cmdTimeoutMs", "timeoutMs: {value}", "timeoutMs: {value}"),
    ColorsCmdTtlSec => ("colors.cmdTtlSec", "ttlSec: {value}", "ttlSec: {value}"),
    ColorsCmdMaxWidth => ("colors.cmdMaxWidth", "maxWidth: {value}", "maxWidth: {value}"),
    ColorsCmdPreserveColors => ("colors.cmdPreserveColors", "preserveColors: {value}", "preserveColors: {value}"),
    ColorsCmdLastError => ("colors.cmdLastError", "error: {value}", "錯誤: {value}"),
    ColorsCmdAdd => ("colors.cmdAdd", "[a] add command", "[a] 新增指令"),
    ColorsCmdEmpty => ("colors.cmdEmpty", "No custom commands — [a] add", "尚無自訂指令 — [a] 新增"),
    ColorsHintCmdName => ("colors.hintCmdName", "Command name (Enter to continue, Esc to cancel)", "指令名稱（Enter 繼續，Esc 取消）"),
    ColorsHintCmdCommand => ("colors.hintCmdCommand", "Shell command (Enter to save, Esc to cancel)", "Shell 指令（Enter 儲存，Esc 取消）"),
    ColorsHintCmdEdit => ("colors.hintCmdEdit", "Edit command (Enter to save, Esc to cancel)", "編輯指令（Enter 儲存，Esc 取消）"),
    ColorsHintCmdDelete => ("colors.hintCmdDelete", "[y] delete cmd:{name}  [any key] cancel", "[y] 刪除 cmd:{name}  [任意鍵] 取消"),
    ColorsHintCmdRemovedRows => ("colors.hintCmdRemovedRows", "removed from {count} rows", "已從 {count} 列移除"),
    ColorsCmdNameInvalid => ("colors.cmdNameInvalid", "name must match [A-Za-z0-9_-]{1,32} and be unique", "名稱必須符合 [A-Za-z0-9_-]{1,32} 且不得重複"),
    ColorsCmdCommandEmpty => ("colors.cmdCommandEmpty", "command must not be empty", "指令不得為空"),
    MainMenuLanguage => ("mainMenu.language", "Language: {value}", "語言: {value}"),
    MainMenuHintMove => ("mainMenu.hintMove", "[↑/↓] move", "[↑/↓] 移動"),
    MainMenuHintOpen => ("mainMenu.hintOpen", "[Enter] open", "[Enter] 開啟"),
    MainMenuHintCycleLanguage => ("mainMenu.hintCycleLanguage", "[←/→] cycle language", "[←/→] 切換語言"),
    MenuRowsSegments => ("menu.rowsSegments", "Rows & Segments", "列 & 區段"),
    MenuColorsTheme => ("menu.colorsTheme", "Colors & Theme", "顏色 & 主題"),
    MenuTemplates => ("menu.templates", "Templates", "範本"),
    MenuSubagentLine => ("menu.subagentLine", "Subagent Line", "子代理列"),
    MenuSettingsInstall => ("menu.settingsInstall", "Settings & Install", "設定 & 安裝"),
    MenuCodexSettings => ("menu.codexSettings", "Codex Settings", "Codex 設定"),
    MenuSaveExit => ("menu.saveExit", "Save & Exit", "儲存並離開"),
    RowsSegmentsHeader => ("rowsSegments.header", "Row {n}/{max} ({layout} layout)", "列 {n}/{max}({layout} 版面)"),
    RowsSegmentsLayoutAuto => ("rowsSegments.layoutAuto", "auto", "自動"),
    RowsSegmentsLayoutFixed => ("rowsSegments.layoutFixed", "fixed", "固定"),
    RowsSegmentsPomodoroValues => ("rowsSegments.pomodoroValues", "(work {work}min)", "(工作 {work}分)"),
    SubagentLineTitle => ("subagentLine.title", "Subagent Line", "子代理列"),
    CommonMoveModeSuffix => ("common.moveModeSuffix", "[move mode]", "[移動模式]"),
    HintsAddA => ("hints.addA", "[a add]", "[a 新增]"),
    HintsInsertI => ("hints.insertI", "[i insert]", "[i 插入]"),
    HintsDeleteD => ("hints.deleteD", "[d delete]", "[d 刪除]"),
    HintsEnterMoveMode => ("hints.enterMoveMode", "[Enter move-mode]", "[Enter 移動模式]"),
    HintsSpaceLayout => ("hints.spaceLayout", "[space layout]", "[space 版面]"),
    HintsTabSwitchRow => ("hints.tabSwitchRow", "[Tab] switch row", "[Tab] 切換列"),
    HintsPomodoroWorkMin => ("hints.pomodoroWorkMin", "[←/→] work minutes", "[←/→] 工作分鐘"),
    HintsPomodoroRefreshSec => ("hints.pomodoroRefreshSec", "[-/+] refresh sec", "[-/+] 刷新秒數"),
    HintsUsageRefreshSec => ("hints.usageRefreshSec", "[←/→] refresh seconds", "[←/→] 刷新秒數"),
    SegmentPickerHintSelect => ("segmentPicker.hintSelect", "[↑/↓] select", "[↑/↓] 選擇"),
    SegmentPickerHintConfirm => ("segmentPicker.hintConfirm", "[Enter] confirm", "[Enter] 確認"),
    SegmentPickerHintCancel => ("segmentPicker.hintCancel", "[Esc] cancel", "[Esc] 取消"),
    ErrorTitle => ("error.title", "ErrorScreen", "錯誤"),
    ErrorHintRerunWizard => ("error.hintRerunWizard", "[1] rerun wizard", "[1] 重跑精靈"),
    ErrorHintFixManually => ("error.hintFixManually", "[2] fix manually (shows path then exits)", "[2] 手動修正(顯示路徑後離開)"),
    CommonTerminalTooSmall => ("common.terminalTooSmall", "terminal too small", "終端機視窗過小"),
    CommonPreview => ("common.preview", "Preview", "預覽"),
    CommonSegmentPicker => ("common.segmentPicker", "Segment Picker", "區段選擇器"),
    CommonMove => ("common.move", "[←/→] move", "[←/→] 移動"),
    CommonConfigurationError => ("common.configurationError", "Configuration error", "設定錯誤"),
    MainMenuTitle => ("mainMenu.title", "Main Menu", "主選單"),
    WizardTitle => ("wizard.title", "Setup Wizard", "設定精靈"),
    WizardHintQuit => ("wizard.hintQuit", "Esc quits wizard", "[Esc] 結束設定精靈"),
    TemplatesLoadNameLabel => ("templates.loadNameLabel", "Template name to load", "要載入的樣板名稱"),
    SettingsWriteDone => ("settingsWrite.done", "Settings written", "設定已寫入"),
    CodexSettingsTitle => ("codexSettings.title", "Codex Settings", "Codex 設定"),
    CodexSettingsSaved => ("codexSettings.saved", "Saved Codex settings", "Codex 設定已儲存"),
    CodexSettingsReloadedForDiff => ("codexSettings.reloadedForDiff", "{error}; configuration was re-read for a fresh diff", "{error}；已重新讀取設定以取得最新差異"),
    CodexSettingsUnavailable => ("codexSettings.unavailable", "Codex config unavailable: {error}", "Codex 設定無法使用：{error}"),
    CodexSettingsNotDetected => ("codexSettings.notDetected", "Codex not detected — install the Codex CLI, or create ~/.codex/config.toml", "未偵測到 Codex — 請安裝 Codex CLI，或建立 ~/.codex/config.toml"),
    CodexSettingsNotLoaded => ("codexSettings.notLoaded", "not loaded", "尚未載入"),
    CodexSettingsHintRetry => ("codexSettings.hintRetry", "[any key] retry [Esc] back", "[任意鍵] 重試 [Esc] 返回"),
    CodexSettingsHintBack => ("codexSettings.hintBack", "[Esc] back", "[Esc] 返回"),
    CodexSettingsSelectTheme => ("codexSettings.selectTheme", "Select theme (Enter confirms):", "選擇主題（Enter 確認）："),
    CodexSettingsConfirmWrite => ("codexSettings.confirmWrite", "Write config.toml changes? [y/n]", "寫入 config.toml 變更？[y/n]"),
    CodexSettingsSimulatedPreview => ("codexSettings.simulatedPreview", "Simulated preview — not Codex's renderer; details may differ.", "模擬預覽——非 Codex 的轉譯器，細節可能不同。"),
    CodexSettingsPreviewUnavailable => ("codexSettings.previewUnavailable", "preview unavailable: {error}", "預覽無法使用：{error}"),
    CodexSettingsHintActions => ("codexSettings.hintActions", "[←/→] select/move [Enter] move mode [e] tmTheme [t] themes [w] write [a/d] items [c] colors [Esc] back", "[←/→] 選擇/移動 [Enter] 移動模式 [e] tmTheme [t] 主題 [w] 寫入 [a/d] 項目 [c] 色彩 [Esc] 返回"),
    ColorsSegmentForeground => ("colors.segmentForeground", "{segment} fg: ", "{segment} 前景色："),
    ColorsPomodoroWorkMin => ("colors.pomodoroWorkMin", "pomodoro work min: {value}", "番茄鐘工作分鐘：{value}"),
    ColorsPomodoroRefreshSec => ("colors.pomodoroRefreshSec", "pomodoro refresh sec: {value}", "番茄鐘刷新秒數：{value}"),
    ColorsUsageRefreshSec => ("colors.usageRefreshSec", "usage refresh sec: {value}", "用量刷新秒數：{value}"),
    TmThemeTitle => ("tmTheme.title", "tmTheme Editor", "tmTheme 編輯器"),
    TmThemeFieldGlobalForeground => ("tmTheme.fieldGlobalForeground", "global foreground", "全域前景色"),
    TmThemeFieldGlobalBackground => ("tmTheme.fieldGlobalBackground", "global background", "全域背景色"),
    TmThemeFieldConstantNumeric => ("tmTheme.fieldConstantNumeric", "constant.numeric (Usage)", "constant.numeric（用量）"),
    TmThemeFieldConstant => ("tmTheme.fieldConstant", "constant (Usage)", "constant（用量）"),
    TmThemeFieldConstantLanguage => ("tmTheme.fieldConstantLanguage", "constant.language (Limit)", "constant.language（限額）"),
    TmThemeFieldStorageType => ("tmTheme.fieldStorageType", "storage.type (Limit)", "storage.type（限額）"),
    TmThemeNone => ("tmTheme.none", "(none)", "（無）"),
    TmThemeFusedSelector => ("tmTheme.fusedSelector", "{field}: fused selector → dedicated selector ({colour})", "{field}：合併選擇器 → 專用選擇器（{colour}）"),
    TmThemeNoColourChanges => ("tmTheme.noColourChanges", "No colour changes.", "沒有色彩變更。"),
    TmThemeSplitScopesFirst => ("tmTheme.splitScopesFirst", "{error}; press s to split the fused Codex scopes first", "{error}；請先按 s 拆分合併的 Codex 範圍"),
    TmThemeSaved => ("tmTheme.saved", "Saved tmTheme", "tmTheme 已儲存"),
    TmThemeReloadedForDiff => ("tmTheme.reloadedForDiff", "{error}; theme was re-read for a fresh diff", "{error}；已重新讀取主題以取得最新差異"),
    TmThemeConfirmWrite => ("tmTheme.confirmWrite", "Write tmTheme colour changes? [y/n]", "寫入 tmTheme 色彩變更？[y/n]"),
    TmThemeNoEditableTheme => ("tmTheme.noEditableTheme", "No editable XML tmTheme selected. Press any key to load.", "未選擇可編輯的 XML tmTheme。按任意鍵載入。"),
    TmThemeHintActions => ("tmTheme.hintActions", "[↑/↓] field [←/→] named colour [e] hex edit [s] split Codex scopes [w] write diff [Esc] back", "[↑/↓] 欄位 [←/→] 命名色彩 [e] 編輯十六進位色碼 [s] 拆分 Codex 範圍 [w] 寫入差異 [Esc] 返回"),
    TmThemeHexColour => ("tmTheme.hexColour", "hex colour: {value}", "十六進位色彩：{value}"),
    TmThemeColourSwatch => ("tmTheme.colourSwatch", "2-cell colour swatch: ██", "雙格色彩樣本：██"),
    TmThemeNamedColour => ("tmTheme.namedColour", "named colour: {name} ({index}/58)", "命名色彩：{name}（{index}/58）"),
    TmThemeCustomColour => ("tmTheme.customColour", "custom colour: (-/58)", "自訂色彩：(-/58)"),
    TmThemeUnavailable => ("tmTheme.unavailable", "tmTheme missing, binary, or malformed (read-only)", "tmTheme 不存在、為二進位檔或格式錯誤（唯讀）"),
    TmThemeInvalidHex => ("tmTheme.invalidHex", "hex colour must be #RRGGBB", "十六進位色彩必須為 #RRGGBB"),
    TmThemeScopesSplit => ("tmTheme.scopesSplit", "Codex scopes split; each may now be edited", "Codex 範圍已拆分；現在可分別編輯"),
}

/// Returns every registered message key.
pub fn all_keys() -> &'static [Key] {
    KEYS
}

/// Looks up and interpolates a TUI message.
pub fn t(lang: Lang, key: &Key, params: &[(&str, &str)]) -> String {
    let template = message(*key, lang);
    let mut rendered = String::with_capacity(template.len());
    let mut remaining = template;

    while let Some(open) = remaining.find('{') {
        rendered.push_str(&remaining[..open]);
        let after_open = &remaining[open + 1..];
        let Some(close) = after_open.find('}') else {
            rendered.push_str(&remaining[open..]);
            return rendered;
        };
        let name = &after_open[..close];
        if !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            && let Some((_, value)) = params.iter().find(|(parameter, _)| *parameter == name)
        {
            rendered.push_str(value);
        } else {
            rendered.push('{');
            rendered.push_str(name);
            rendered.push('}');
        }
        remaining = &after_open[close + 1..];
    }

    rendered.push_str(remaining);
    rendered
}

#[cfg(test)]
mod tests {
    use super::{Key, Lang, all_keys, t};

    /// REQ-04 / S-09
    #[test]
    fn test_s09_cmd_keys_both_langs() {
        let keys = [
            Key::ColorsCmdCommand,
            Key::ColorsCmdTimeoutMs,
            Key::ColorsCmdTtlSec,
            Key::ColorsCmdMaxWidth,
            Key::ColorsCmdPreserveColors,
            Key::ColorsCmdAdd,
            Key::ColorsCmdEmpty,
            Key::ColorsHintCmdName,
            Key::ColorsHintCmdCommand,
            Key::ColorsHintCmdEdit,
            Key::ColorsHintCmdDelete,
            Key::ColorsHintCmdRemovedRows,
            Key::ColorsCmdNameInvalid,
            Key::ColorsCmdCommandEmpty,
        ];

        for key in keys {
            assert!(!t(Lang::En, &key, &[]).is_empty());
            assert!(!t(Lang::ZhTw, &key, &[]).is_empty());
            assert!(all_keys().contains(&key));
        }
    }
}
