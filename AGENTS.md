# Cosmos Term Agent Instructions

## Mission

Maintain Cosmos Term as a standalone WezTerm fork with an integrated,
pane-aware filesystem explorer. Preserve the terminal core and keep changes
near explicit Cosmos integration seams.

## Startup

Read `SESSION-HANDOFF.md` and `memory/LEARNINGS.md` before changing code.
Use the repository root as the workspace. Treat
`navilan-terminal-workspace-product-vision.docx` as product input and preserve
it.

## Safety boundaries

- Do not target, stop, reconfigure, or reuse the user's installed WezTerm.
- Do not attach tests to the user's default tmux server. Use a unique socket
  with `tmux -S /tmp/<unique>.sock` for all mutating tmux tests.
- Keep Cosmos config, data, cache, socket, logs, bundle ID, and protocol
  environment names distinct from WezTerm.
- Do not commit generated `dist/` bundles, secrets, logs, runtime state, raw
  sessions, or personal data.
- Preserve upstream attribution and license files.

## Architecture

- Put reusable explorer state, directory, watcher, and pane-context logic in
  `cosmos-workspace`.
- Put native terminal-window rendering and input integration in
  `wezterm-gui/src/termwindow/cosmos.rs`.
- Keep edits to upstream modules narrow and explain compatibility deviations
  in `docs/cosmos-architecture.md` and `memory/LEARNINGS.md`.
- Keep directory scans, pane resolution, and filesystem events off the render
  thread.

## Verification

Run at minimum:

```sh
cargo test -p cosmos-workspace
cargo check -p wezterm-gui -p wezterm -p wezterm-mux-server
git diff --check
```

For release or renderer changes, also build/package the release app, verify
its signature and plist, and exercise the live matrix in
`docs/cosmos-testing.md`. Confirm the installed WezTerm process and default
tmux clients are unchanged before and after isolation tests.

## Handoff and Git

Keep `SESSION-HANDOFF.md` current with verified state and remaining risks. Add
only stable reusable facts to `memory/LEARNINGS.md`.

Use personal GitHub identity `NavilanSanthanakrishnan` and Git author
`Navilan Santhanakrishnan
<143132458+NavilanSanthanakrishnan@users.noreply.github.com>` unless repository
instructions explicitly change it. Verify the GitHub login before access.
