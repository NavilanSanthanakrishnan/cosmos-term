# Cosmos Term Session Handoff

Updated: 2026-07-16

## Current state

V1 is implemented, packaged, and installed at
`/Applications/Cosmos Term.app`. The source is published in the private
repository `NavilanSanthanakrishnan/cosmos-term` on branch `main`.

Cosmos Term is a native WezTerm fork, not a wrapper. It retains terminal tabs,
splits, rendering, Lua configuration, and mux behavior while adding the left
explorer and using independent application/runtime identities.

## Verified behavior

- Signed arm64 macOS bundle launches as `Cosmos Term` with bundle ID
  `com.navilan.cosmos-term`.
- The explorer renders and remains resizable beside normal terminal content.
- The explorer now matches the supplied VS Code reference: Code OSS Dark
  Modern colors, a 35 px title, 22 px rows, 8 px indentation, proportional
  Helvetica labels, native chevrons, and colored Seti/Nerd file icons.
- The 420 px default remains intentionally roomy, with 14 pt terminal text and
  a 100 × 32 initial terminal, while the Explorer itself uses VS Code's compact
  density and pixel-snapped glyph placement.
- The Activity Bar, visible mode badge, and custom toolbar were removed. The
  default header is `EXPLORER` plus one ellipsis, matching the reference.
- The explorer is permanently visible and `Command+Shift+E` is an inert `Nop`.
  With explorer focus, `L` toggles Follow ↔ Locked and the ellipsis turns blue
  while Locked; clicking a terminal pane reliably returns keyboard focus.
- Dotfiles are visible by default, while `.git` and `.DS_Store` follow VS Code
  exclusions. Git status is loaded on a worker and rendered as right-aligned,
  color-coded file decorations such as `M`.
- In Follow mode, native `cd` changes make the new CWD the sole visible root;
  parent and sibling folders are excluded. Project Follow scopes to the Git
  root, while Locked holds the displayed root.
- Switching focus between native split panes follows each pane's CWD.
- Creating and removing a directory under an expanded root updates live.
- Sidebar width, roots, expanded directories, and follow mode survive restart.
- Locked mode preserves the tree expansion while the terminal CWD changes.
- A two-pane tmux session on a dedicated socket reports source `tmux` and
  follows tmux pane selection from one directory to another.
- Existing WezTerm and default tmux clients remained unchanged during testing.
- A hostile inherited `WEZTERM_CONFIG_FILE` and `WEZTERM_UNIX_SOCKET` do not
  redirect Cosmos Term; its bundled config and Cosmos socket are used.
- Parent `WEZTERM_*` protocol/config variables and stale `TMUX`/`TMUX_PANE`
  attachment values are absent from newly spawned Cosmos terminal shells.
- `Command+W` is a direct, unconfirmed `CloseCurrentTab`; a live two-tab test
  closed only the active tab. `Command+Q` retains protected autosave/close.
- Folder-scoped row generation has unit coverage for excluding both saved
  sibling roots and parent-directory siblings; Git porcelain parsing and
  layout migration are covered as well.

## Verification commands

```sh
cargo test -p cosmos-workspace
cargo check -p wezterm-gui -p wezterm -p wezterm-mux-server
cargo build --release -p wezterm-gui -p wezterm -p wezterm-mux-server
ci/package-cosmos-macos.sh
codesign --verify --deep --strict "dist/Cosmos Term.app"
plutil -lint "dist/Cosmos Term.app/Contents/Info.plist"
git diff --check
```

## Important paths

- Explorer core: `cosmos-workspace/src/lib.rs`
- Window integration: `wezterm-gui/src/termwindow/cosmos.rs`
- Bundled config: `assets/cosmos/cosmos.lua`
- App template: `assets/macos/Cosmos Term.app`
- Package script: `ci/package-cosmos-macos.sh`
- Persistent explorer state:
  `~/Library/Application Support/cosmos-term/workspace-state.json`
- Runtime sockets: `~/Library/Caches/cosmos-term/runtime`

## Baseline and compatibility

The fork starts from WezTerm commit
`5046fc225992db6ba2ef8812743fadfdfe4b184a`, matching the installed WezTerm
baseline. Three narrow modern-toolchain compatibility changes are intentional:

- FreeType bindings avoid constructing Rust slices from null pointers.
- The obsolete macOS full-screen button constant is omitted.
- The legacy `glium` package is compiled at optimization level 0 in release
  builds because current LLVM miscompiles that old version's OpenGL texture
  path; the rest of the release remains optimized.

## Remaining release work

The local bundle is ad-hoc signed and is not notarized. Automated macOS CI,
release artifacts, and migration to a newer upstream WezTerm baseline are
future work, not V1 blockers.
