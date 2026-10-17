# Changelog

All notable changes to phosphorpulse are listed here, newest first.
The format follows [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [0.3.0] - 2026-10-17

### Changed

- **Breaking:** the subagent status line no longer uses `segments.<id>.fg` / `.bg`. It reads only the new `subagent.colors.<id>.fg` / `.bg`. To keep your old colours, copy each `segments.<id>` colour to `subagent.colors.<id>`.
- **Breaking:** a `gauge.okColor`, `gauge.warnColor` or `gauge.hotColor` value that is not `#RRGGBB` now fails config validation. Before, it was ignored.
- TUI colour swatches show the active built-in theme's colours instead of fixed matrix-tron colours. A custom template falls back to matrix-tron, the same as the renderer.

### Added

- `subagent.colors.<id>.fg` / `.bg` config keys, and a "Subagent" section on the TUI colour screen.
- This changelog.

### Fixed

- `gauge.okColor`, `gauge.warnColor` and `gauge.hotColor` now change the rendered gauge segments (`limit5h`, `limit7d`, `limitModel`, `ctx`, `cache`, subagent `ctx`). Before, they changed only the TUI swatch. The ok colour resolves as `segments.<id>.fg` (or a row's `color.fg`; `subagent.colors.ctx.fg` on the subagent line) > `gauge.okColor` > theme. Values must be `#RRGGBB`.
- Template import accepts `subagent.colors` and `gauge.okColor` / `warnColor` / `hotColor`, so a template saved with these keys imports again.
- Template import rejects a colour that is not `#RRGGBB` (segment, row, subagent and gauge colours). Before, such a template imported and then left the status line showing a config error.

## [0.2.9] - 2026-10-17

### Added

- The prompt-cache segment is available in the TUI.

### Fixed

- Implausible timestamps are no longer trusted.
- Background refreshes coordinate through kernel file locks.

## [0.2.8] - 2026-10-16

### Fixed

- The usage refresher refetches when the stored next-fetch time is implausibly far in the future, instead of waiting for it.

## [0.2.7] - 2026-10-11

### Added

- Prompt-cache countdown segment and a `cache-state` subcommand.
- Subagent line: role icons by agent type and an activity sparkline.

### Fixed

- Version digits in subagent model names are joined correctly.

## [0.2.6] - 2026-09-30

### Fixed

- `pomodoro-gate` lets Claude Code system notifications through, and also lets prompts through when it cannot read the stdin.
- Gauge warn, hot and dim colours now win over a configured segment `fg`.

## [0.2.5] - 2026-09-29

### Added

- `pomodoro-gate` subcommand for Claude Code's `UserPromptSubmit` hook. When `pomodoro.blockDuringRest` is true, it blocks interactive prompts during short and long breaks and prints the remaining time. It fails open on any read, parse or validation error, on non-interactive entrypoints, and on clock regression.
- A block-during-break toggle on the Colors & Themes screen, after the work-minutes knob.
- On save and exit, the TUI adds or removes its own `UserPromptSubmit` hook entry to match `pomodoro.blockDuringRest`. This happens in the same write as the statusline update, with a single backup. Other hooks are preserved, malformed hook shapes are refused, the save aborts if the file changed meanwhile, and sync errors stay on screen.

## [0.2.4] - 2026-09-28

### Fixed

- The usage refresher reads the macOS Keychain item for the current user (`-a $USER`) first, then falls back to the service-only lookup. A second, stale item from an old login no longer makes the gauge look permanently rate-limited.
- Expired OAuth tokens are never sent to the usage API.

## [0.2.3] - 2026-09-24

### Added

- `usage.source` (`auto` | `local` | `api`, default `auto`). In `auto`, the refresher reads Claude Code's cached usage from `~/.claude.json` when it is at most 15 minutes old and the account matches, and calls the API otherwise. `local` never calls the API.

### Changed

- When no weekly limit is active for the current model, `limitModel` shows the first per-model weekly limit, dimmed, instead of `--`.

### Fixed

- A 429 with a `retry-after` below 60 s (the API sends 0) now backs off 300 s instead of 1 s, which kept hitting the endpoint and prolonged the lockout.

## [0.2.2] - 2026-09-19

### Fixed

- On Linux, the custom-command timeout now kills the whole process group. The previous `/bin/kill` call failed silently with procps-ng; it now uses `killpg` directly.

## [0.2.1] - 2026-09-19

### Fixed

- A custom-command timeout kills the whole process group, so a grandchild holding stdout is no longer leaked.
- Up to 32 KiB of the forwarded stdin is written before render exits, so a command reading stdin no longer sees an empty input.
- Custom-command failures are visible: stderr is captured, the cache records a `lastError`, and Colors & Theme shows it under the command row.
- Cache cleanup deletes only that command's own cache files, matched by exact name.
- The default `fg` of a `cmd:` segment follows the active theme palette.

## [0.2.0] - 2026-09-19

### Added

- `cmd:<name>` segments that show the output of a shell command declared in the `commands` map of `settings.json`. Output is cached per command and directory and refreshed in the background, with a timeout and an output cap. The Colors & Theme screen can add, edit and delete commands; the preview never runs them.
- `limitModel` segment: the weekly per-model limit from the OAuth usage API, with a gauge and reset countdown. It dims when the data is over 30 minutes old and shows `--` when it has expired.
- `phosphorpulse usage-refresh` subcommand, started in the background by render. It reads the token from `PPULSE_USAGE_TOKEN_FILE`, `~/.claude/.credentials.json` or the macOS Keychain.
- `usage.refreshSec` setting (60–600, default 300) on the Colors & Themes screen.
- `session`, `fastMode`, `outputStyle` and `thinking` segments, read from the statusline stdin. They hide at their default values, cap text at 24 columns, and switch between Nerd Font and ASCII glyphs with the `nerdFont` flag.
- The TUI preview mirrors the real layout: main header, blank line and subagent bullet.

### Changed

- Built-in templates show `limitModel` in their default rows, and `burn` is no longer in the defaults.
- Segment picker and colour lists scroll to keep the selection visible.

## [0.1.0] - 2026-08-16

### Added

- Full-screen TUI built with ratatui: main menu, rows and segments editor, colors and theme, templates (import, export, delete), Codex settings (`config.toml` and a `.tmTheme` editor with scope split), and settings and install (`[w]` writes the statusline entries, `[b]` installs the binary).
- First-run setup wizard.
- English and 繁體中文 interface, README, install guide and themes guide.

### Fixed

- The subagent name is read from the transcript's sibling `.meta.json` file, which is where Claude Code actually supplies it.

## [0.0.1] - 2026-08-15

### Added

- First release: `phosphorpulse render`, a Rust statusline renderer whose output matches phosphorflux byte for byte.
- Three built-in themes, segments and configurable rows.
- Config loader with `PPULSE_*` environment overrides. Invalid config fails loudly.
- External lookups with hard timeouts and a lookups cache.
- Pomodoro segment with throttled notifications.
- `phosphorpulse migrate`, which copies a whitelist of settings from phosphorflux.
- Prebuilt binaries for four targets, with SHA-256 checksums.

[0.3.0]: https://github.com/twjohnwu/phosphorpulse/compare/v0.2.9...v0.3.0
[0.2.9]: https://github.com/twjohnwu/phosphorpulse/compare/v0.2.8...v0.2.9
[0.2.8]: https://github.com/twjohnwu/phosphorpulse/compare/v0.2.7...v0.2.8
[0.2.7]: https://github.com/twjohnwu/phosphorpulse/compare/v0.2.6...v0.2.7
[0.2.6]: https://github.com/twjohnwu/phosphorpulse/compare/v0.2.5...v0.2.6
[0.2.5]: https://github.com/twjohnwu/phosphorpulse/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/twjohnwu/phosphorpulse/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/twjohnwu/phosphorpulse/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/twjohnwu/phosphorpulse/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/twjohnwu/phosphorpulse/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/twjohnwu/phosphorpulse/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/twjohnwu/phosphorpulse/compare/v0.0.1...v0.1.0
[0.0.1]: https://github.com/twjohnwu/phosphorpulse/releases/tag/v0.0.1
