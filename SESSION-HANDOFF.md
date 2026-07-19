# Cosmos Term Session Handoff

Updated: 2026-07-18

## Current state

V1 is implemented, packaged, and installed at
`/Applications/Cosmos Term.app`. The source repository is public at
`https://github.com/NavilanSanthanakrishnan/cosmos-term`; the product launch
and lightweight system-capacity footer are merged on `main`. The first public
Apple-silicon prerelease is
`https://github.com/NavilanSanthanakrishnan/cosmos-term/releases/tag/v0.1.0-alpha.1`.

Cosmos Term is a native WezTerm fork, not a wrapper. It retains terminal tabs,
splits, rendering, Lua configuration, and mux behavior while adding the left
explorer and using independent application/runtime identities.

## Verified behavior

- Signed arm64 macOS bundle launches as `Cosmos Term` with bundle ID
  `com.navilan.cosmos-term`.
- The application now uses a dark galaxy/black-hole icon with a violet-cyan
  accretion ring and subtle terminal mark. The 1024 px PNG is the source of
  truth, and `assets/icon/build-cosmos-macos.sh` reproducibly generates the
  bundled macOS ICNS.
- Native, SSH, tmux, and synthetic pane terminals use `Cosmos Term` as their
  initial program identity. Foreground process names and application-provided
  terminal titles continue to replace that placeholder normally.
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
  directory scans or rebuild unchanged rows. Cached expanded folders load
  once, watcher/periodic events perform explicit refreshes, pane metadata uses
  the non-blocking stale-cache policy, and worker responses are collected
  every 50 ms only while work is pending before the timer backs off to 500 ms.
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
- `COSMOS_TERM_DATA_DIR` and `COSMOS_TERM_RUNTIME_DIR` provide explicit
  disposable roots for isolated release testing without sharing the installed
  application's state or sockets.
- Parent `WEZTERM_*` protocol/config variables and stale `TMUX`/`TMUX_PANE`
  attachment values are absent from newly spawned Cosmos terminal shells.
- `Command+W` opens a password-masked `COSMOS TERM CLOSE LOCK` overlay, verifies
  the existing PBKDF2-HMAC-SHA256 close-lock credential in-process, and runs
  the pre-close autosave before permanently closing the current tab and all of
  its panes. Escape or failed verification blocks the close. The password is
  not logged or passed through process arguments. `Command+Q` retains the
  equivalent protected whole-application autosave/close flow.
- Protected close is capability-detected in the public bundled config. Existing
  users with a close-lock file retain the password/autosave flow; a clean
  installation uses confirmed `Command+W` and normal `Command+Q` without a
  tmux-manager dependency. Both bundled-config modes were verified with
  `show-keys`.
- The external tmux-manager AppleScript dialog and `tmux Manager` notification
  identity are no longer used. Canceling is silent; any failure toast is
  branded `Cosmos Term`. Overlay cancellation also restores the underlying
  pane title.
- Live cancellation was verified after typing a masked value. The complete
  success path was verified with a temporary PBKDF2 credential, dedicated tmux
  socket, and isolated manager state: the workspace snapshot was written and
  the disposable tab/app closed. The temporary server and state were removed.
- The exact protected-close state before footer work is retained at annotated
  tag `backup/command-w-protected-2026-07-16` (commit `7abaf9cae`).
- A native 22 logical-pixel status bar now reserves real space below both the
  Explorer and terminal. It follows Code OSS Dark Modern status styling:
  `#181818` background, `#2B2B2B` top border, `#CCCCCC` 12 pt system UI text,
  a green live indicator, and compact left/right item alignment.
- The footer shows structured Codex rate-limit usage, the nearest reset, and
  the number of exact `codex` executables. A live test moved the loop count
  from four to five and back to four without an app restart; usage advanced
  from 48% to 49% while the final build remained open.
- The footer also shows system-wide CPU and meaningful occupied/total RAM,
  using cumulative host CPU ticks and Mach VM statistics. It reuses the
  pane-context worker, caches the host port/page size/total RAM, and samples a
  stable ten-second rolling view. No new timer, worker thread, helper,
  subprocess, daemon, filesystem poll, or network request was added.
- Capacity rendering invalidates the window only when the formatted label
  changes. A first implementation that repainted every two seconds averaged
  0.663% idle CPU and was rejected; the final isolated build averaged 0.420%
  across 30 steady-state samples, below the established 0.48% optimized
  reference. It settled at 87.4 MiB current footprint and 245.4 MiB peak
  versus the matched pre-feature run's 80 MiB and 242 MiB. Its only direct
  child was the requested `/bin/zsh -f`.
- Codex status runs through the existing pane-context worker and native macOS
  process enumeration. It creates no daemon, helper, launch agent, shell
  poller, `ps`, or `pgrep` process. Active-rollout metadata is checked every
  two seconds, content is read only after it changes, and broader discovery
  across the 4 GB local session history is cached for 15 seconds. Discovery
  keeps only the 16 newest candidates and native process counting reuses one
  path buffer across all PIDs.
- Explorer UI fonts now resolve directly from WezTerm's registered built-in
  font map. The prior generic CoreText route enumerated and parsed the complete
  macOS font catalog for each UI size/weight variant and was responsible for
  the abnormal startup high-water mark.
- A fresh installed launch now measures 81.8 MiB current physical footprint
  and 237.6 MiB peak, down from the previous 954–956 MiB peak. The optimized
  app averages 0.48% CPU across 30 steady-state idle samples (0.0–1.7%),
  compared with 1.21% for the previous Cosmos build and 0.54% for the matched
  fresh WezTerm baseline. The remaining memory above WezTerm is consistent
  with Cosmos's native Explorer, Seti/UI fonts, status bar, and wider WebGPU
  surface rather than retained font-catalog allocations.
- Folder-scoped row generation has unit coverage for excluding both saved
  sibling roots and parent-directory siblings; Git porcelain parsing and
  layout migration are covered as well. Structured Codex usage parsing,
  nearest-reset selection, executable-name filtering, CPU tick deltas, compact
  capacity labels, and live native macOS capacity reads are also covered.
- The final installed `cosmos-term-gui` SHA-256 is
  `b1f533b3462ab7681efc03a00a7813fb26799f0f7f8ca7fc84e2d38455472369`;
  it exactly matches the packaged release binary.
  The final native close-lock capture is
  `/tmp/cosmos-visual/cosmos-close-lock-final.png`
  (`5d38d0c72f9ad7e7c97f95b2449148b9d3fd691fd751107d7507864e64b523dd`,
  2486 × 1702 at Retina resolution).
- The public README screenshot is
  `docs/screenshots/cosmos-term-workbench.png`
  (`50d745b0b1769e5ec7f2cb4393e1fef954db969c755484b0206176b03678da12`,
  1148 × 807). It shows the native formatted architecture Markdown workspace,
  Explorer, terminal-return control, edit control, tabs, and CPU/RAM footer.
  It was captured from an isolated packaged process; the
  installed Cosmos PID and default tmux clients were unchanged before and
  after the capture.
- Public prerelease `v0.1.0-alpha.1` targets merge commit
  `000b4bcfc91457d0ca9e8d0319630a814cb342ef`. Its 52,182,993-byte
  `Cosmos-Term-macos-arm64.zip` re-downloaded with SHA-256
  `ce0f160ebd6747a8bb40c3b115f6478b8cb22c9acb26b53b5b316ad7fe22cff8`.
  GitHub reports both the ZIP and portable checksum assets uploaded, and the
  archive passed `unzip -t`.
- `Command+S` toggles only the right-side presentation between the focused
  terminal/tmux pane and its file workspace. A new native/tmux pane context
  opens a blank `FILE WORKSPACE` with no selected file and no search field;
  the user must choose a visible file from the left Explorer. A clean
  workspace resets when the focused pane or its resolved CWD changes. The
  terminal/tmux process remains alive underneath throughout.
- Explorer file activation uses the native loader. Markdown is rendered with
  headings, lists, quotes, code, rules, task markers, visible link
  destinations, width-aware prose wrapping, common HTML cleanup, and table
  rows; other UTF-8 files use a code-oriented view.
- `Command+E` toggles preview/edit, `Command+Return` performs a same-directory
  atomic save, Escape moves from edit to preview and preview to the live
  terminal, and `Command+Shift+D` deliberately discards edits. The header
  identifies the active path and exposes `‹ TERMINAL`, `EDIT`, and `PREVIEW`
  controls. File loads are limited to 2 MiB, reject binary/out-of-root paths,
  and saves refuse to overwrite a file whose revision changed externally.
- Load and save run through the on-demand file workspace worker, which sleeps
  on its channel while idle. The release test process had no new
  helper/daemon; its only child was the requested shell.
- The isolated signed release completed blank panel → terminal → blank panel →
  open → edit → type → atomic save with the GUI process continuously alive.
  Changing its shell from the workspace root to `docs/` reset the panel to the
  `DOCS` Explorer root with no selected file. The workspace
  suite now has 23 passing tests, including out-of-root rejection, atomic
  save cleanup, and external-revision conflict. Full GUI/CLI/mux checks,
  release packaging, strict signature verification, plist validation, and
  `git diff --check` pass. The repository-wide format check still reports only
  the pre-existing `window/src/os/macos/app.rs` difference.
- Before this replacement, the installed formatted-file-workspace build was
  copied to
  `~/Applications/Cosmos Term Releases/Cosmos Term-before-command-s-toggle-2026-07-18.app`
  (GUI SHA-256 `0ad853d88dd45511ab71cb7676e6434d150db85d96794d088d7b2a990b532f0a`).
  The signed toggle candidate is staged as
  `Cosmos Term-command-s-toggle-2026-07-18.app` in the same directory (GUI
  SHA-256 `57502ba40b2b7217692dbe3dd1e578de80100c5029bbbcdd9285be355ffe641f`).
  That exact binary is installed at `/Applications/Cosmos Term.app`. Installed
  PID 65297 launched in the repository; the first live `Command+S` displayed
  `FILE WORKSPACE` with no selected file or search field, and the second
  `Command+S` restored the same `zsh` pane at the same CWD. PID 65297 and its
  shell child remained continuously alive. The installed empty-panel capture
  is `/tmp/cosmos-file-workspace-capture/installed-command-s-empty.png`
  (SHA-256
  `efa2ad09ff1a9642bf3e4cfb69654197b133159f572b5a1a0dc4541568b56bf9`).

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
- Registered Explorer UI fonts use a built-in-only resolution entry point
  before the generic platform locator, avoiding full CoreText catalog
  enumeration while preserving normal fallback behavior for unknown fonts.

## Remaining release work

- The local bundle is ad-hoc signed and is not notarized.
- Automated release packaging and migration to a newer upstream WezTerm
  baseline remain future work.
