# Changelog

All notable Cosmos-specific changes are documented here. Upstream terminal-core
history remains available in `docs/changelog.md` and the retained Git history.

## Unreleased

## 0.1.0-alpha.2 — 2026-07-20

- Added a native right-side file workspace without replacing the live
  terminal/tmux pane.
- Added a pane-aware `Command+S` terminal/file-workspace toggle and
  click-to-open Explorer files; the file workspace starts with no selected
  file and has no search surface.
- Scoped the file-workspace overlay to the focused native pane or exact active
  tmux pane rectangle, leaving every other pane live and visible.
- Anchored the file workspace to the tmux pane where it was opened, so focus
  commands no longer make it jump sides while real swaps and resizes still
  follow the owning pane.
- Made tmux file preview keyboard-transparent, so command prompts, copy mode,
  key tables, window and pane commands, repeat bindings, and direct no-prefix
  bindings continue working; configured prefixes also remain available while
  editing.
- Added global Explorer keyboard navigation with `<tmux prefix> 0`, W/S
  one-row movement, Shift+W/Shift+S five-row movement, and Return activation.
  The prefix is buffered so 0 never leaves tmux waiting in its prefix table,
  while every other command—including positional 1/2—exits navigation and is
  replayed through tmux unchanged.
- Made the five-row shortcuts accept macOS's normalized uppercase W/S events,
  where the character can retain the Shift meaning after the modifier flag is
  removed by the text-input pass.
- Added W/S and Shift+W/Shift+S vertical file-preview scrolling plus A/D and
  Shift+A/Shift+D horizontal scrolling, while preserving real tmux prefix
  commands.
- Added formatted Markdown preview, UTF-8 text preview, line-numbered editing,
  native path/mode navigation, and immediate terminal return.
- Added 2 MiB file limits, workspace-boundary enforcement, atomic saves,
  permission preservation, external-revision conflict detection, and
  unsaved-close protection.
- Moved explicit file saving to `Command+Return`.
- Added fail-closed preferred-response handling for three exact Codex
  model-choice prompts. Observe mode is the default; active mode supports
  native panes and exact revalidated tmux targets without changing focus.
- Added immediate prompt-automation controls, metadata-only owner-private
  audit records, manual-input pause, and process-wide deduplication.
- Capped the bundled renderer at 30 FPS and moved prompt inspection behind a
  200 ms output quiet period to reduce GLX resume/streaming contention.
- Removed the GitHub Actions workflow and Dependabot schedule; project
  verification and dependency review are local.

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
