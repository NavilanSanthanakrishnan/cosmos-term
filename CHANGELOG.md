# Changelog

All notable Cosmos-specific changes are documented here. Upstream terminal-core
history remains available in `docs/changelog.md` and the retained Git history.

## Unreleased

- Added a native right-side file workspace without replacing the live
  terminal/tmux pane.
- Added `Command+P` Quick Open and click-to-open Explorer files.
- Added formatted Markdown preview, UTF-8 text preview, line-numbered editing,
  native path/mode navigation, and immediate terminal return.
- Added 2 MiB file limits, workspace-boundary enforcement, atomic saves,
  permission preservation, external-revision conflict detection, and
  unsaved-close protection.

## 0.1.0-alpha.1 — 2026-07-18

- Added a permanent, resizable Code OSS-style filesystem Explorer.
- Scoped the Explorer to the focused pane's exact working directory.
- Added native-pane and isolated tmux-pane context following.
- Added lazy directory loading, filesystem watches, and Git decorations.
- Added the Dark Modern status bar with local Codex usage and loop count.
- Added lightweight system-wide CPU and RAM capacity to the existing status
  refresh, using native macOS counters with no helper process or extra thread.
- Added optional password-protected, autosaved close behavior.
- Added independent Cosmos configuration, data, runtime, socket, and bundle
  identities so Cosmos Term can run beside WezTerm.
- Added mixed-DPI rendering fixes and bounded startup/idle resource work.
- Added the dark galaxy black-hole application icon.
