# Cosmos Term Session Handoff

Updated: 2026-07-16

## Current state

V1 is implemented, packaged, and installed at
`/Applications/Cosmos Term.app`. The source workspace is the private
repository `NavilanSanthanakrishnan/cosmos-term` on branch `main`.

Cosmos Term is a native WezTerm fork, not a wrapper. It retains terminal tabs,
splits, rendering, Lua configuration, and mux behavior while adding the left
explorer and using independent application/runtime identities.

## Verified behavior

- Signed arm64 macOS bundle launches as `Cosmos Term` with bundle ID
  `com.navilan.cosmos-term`.
- The explorer renders permanently beside normal terminal content and remains
  resizable.
- The Explorer styling is sourced from Code OSS and the supplied reference:
  `#252526` background, `#2B2B2B` section border, `#37373D` inactive
  selection, 35 px title, 22 px rows, 20 px title inset, 8 px tree indentation,
  13 px title text, 15 px body text in the macOS system UI face, native
  chevrons, and a 10 px dark scrollbar.
- The exact Code OSS `seti.woff` was converted and bundled as
  `VSSeti-Regular.ttf`, with its MIT license retained. File glyph codepoints and
  colors match the Seti icon-theme source instead of approximating them with a
  Nerd Font. The source reference was Code OSS commit
  `77f74446ac8fe1910f56b88d6ac3acdb1ac827e1`.
- The older WezTerm locator does not enumerate CoreText's private
  `-apple-system` alias, so Cosmos registers the OS-owned
  `/System/Library/Fonts/SFNS.ttf` face as `System Font` without bundling or
  redistributing it.
- The 520 logical-pixel default remains intentionally roomy, with 14 pt
  terminal text and a 100 × 32 initial terminal. Complete Explorer geometry is
  DPI-scaled, so it retains the same apparent size at 72 and 144 DPI.
- Layout version 4 migrates previously persisted narrow widths (355 px in the
  live upgrade) to 520 px and saves the migration immediately. The divider
  remains resizable from 240 through 840 logical pixels.
- Explorer typography is scaled independently from width: the header is 13 pt
  and tree labels are 15 pt. The live user-resized 263 px width was preserved
  while applying the larger text.
- Explorer paint and repeated pane-context checks no longer queue redundant
  directory scans. Cached expanded folders load once, watcher/periodic events
  perform explicit refreshes, pane metadata uses the non-blocking stale-cache
  policy, and worker responses are collected every 50 ms.
- macOS initial geometry now uses the active screen's effective DPI. After
  Cocoa finalizes cross-screen placement, Cosmos restores the mux-requested
  cell geometry against the actual screen and resizes WebGPU before repainting;
  repeated mixed-DPI launches remain 100 × 32 instead of opening at 2× size.
- The bundled config uses WebGPU. Native screenshots on the DELL 72 DPI
  display produce exact `#252526` and `#37373D` pixels; the Retina capture
  remains sharp at 2× physical resolution.
- The Activity Bar, visible mode badge, and custom toolbar were removed. The
  default header is `EXPLORER` plus one ellipsis, matching the reference.
- The explorer is permanently visible and `Command+Shift+E` is an inert `Nop`.
  There is no hide, lock, or scope key binding. With Explorer focus, `L`
  reaches the shell as literal input; clicking a terminal pane reliably
  returns keyboard focus.
- Dotfiles are visible by default, while `.git` and `.DS_Store` follow VS Code
  exclusions. Git status is loaded on a worker and rendered as right-aligned,
  color-coded file decorations such as `M`.
- Native `cd` changes make the active pane's exact CWD the sole visible root;
  persisted historical roots, parents, and siblings are excluded.
- Startup waits for active pane context rather than flashing a saved root.
  Directory loading requests the current root first and only then requests
  expanded descendants reachable through the loaded tree.
- Switching focus between native split panes follows each pane's CWD.
- Creating and removing a directory under an expanded root updates live.
- Sidebar width and reachable expanded directories survive restart without
  overriding current-folder scope.
- A two-pane tmux session on a dedicated socket reports source `tmux` and
  follows tmux pane selection from one directory to another.
- Existing WezTerm and default tmux clients remained unchanged during testing.
- A hostile inherited `WEZTERM_CONFIG_FILE` and `WEZTERM_UNIX_SOCKET` do not
  redirect Cosmos Term; its bundled config and Cosmos socket are used.
- Parent `WEZTERM_*` protocol/config variables and stale `TMUX`/`TMUX_PANE`
  attachment values are absent from newly spawned Cosmos terminal shells.
- `Command+W` uses the custom close-lock verifier and pre-close autosave before
  permanently closing the current tab and all of its panes. Canceling or
  failing verification blocks the close. `Command+Q` retains the equivalent
  protected whole-application autosave/close flow.
- Folder-scoped row generation has unit coverage for excluding both saved
  sibling roots and parent-directory siblings; Git porcelain parsing and
  layout migration are covered as well.
- The final installed `cosmos-term-gui` SHA-256 is
  `afcd7661981311cb37cbc621fd47f172bf5335c5f081d380a119507470e67833`;
  it exactly matches the packaged release binary.
  Final native captures are
  `/tmp/cosmos-visual/cosmos-explorer-larger-text-final-1x.png`
  (`46a640f3c6b79d40bf79bf64340ac65d09aefd8e21d1e864f011dca10f0f7368`,
  1080 × 717 at 72 DPI) and
  `/tmp/cosmos-visual/cosmos-explorer-larger-text-final-retina.png`
  (`777cd2a9d7e3470e0830608b8c886702ce58b55fbae0510cd6d1c9cd15b6ccfa`,
  2262 × 1434 at 144 DPI).

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
- Bundled Seti font: `assets/fonts/VSSeti-Regular.ttf`
- Seti license: `assets/fonts/LICENSE_SETI.txt`
- App template: `assets/macos/Cosmos Term.app`
- Package script: `ci/package-cosmos-macos.sh`
- Persistent explorer state:
  `~/Library/Application Support/cosmos-term/workspace-state.json`
- Runtime sockets: `~/Library/Caches/cosmos-term/runtime`

## Baseline and compatibility

The fork starts from WezTerm commit
`5046fc225992db6ba2ef8812743fadfdfe4b184a`, matching the installed WezTerm
baseline. Four narrow modern-toolchain/runtime compatibility changes are
intentional:

- FreeType bindings avoid constructing Rust slices from null pointers.
- The obsolete macOS full-screen button constant is omitted.
- The legacy `glium` package is compiled at optimization level 0 in release
  builds because current LLVM miscompiles that old version's OpenGL texture
  path; the rest of the release remains optimized.
- Initial macOS window geometry is converted from physical pixels to AppKit
  points using the active screen DPI, then reconciled once Cocoa has finalized
  mixed-DPI placement.

## Remaining release work

The local bundle is ad-hoc signed and is not notarized. Automated macOS CI,
release artifacts, and migration to a newer upstream WezTerm baseline are
future work, not V1 blockers.
