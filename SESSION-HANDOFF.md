# Cosmos Term Session Handoff

Updated: 2026-07-20

## Current state

V1 is implemented, packaged, and installed at
`/Applications/Cosmos Term.app`. The source repository is public at
`https://github.com/NavilanSanthanakrishnan/cosmos-term`; the product launch
and lightweight system-capacity footer are merged on `main`. The latest public
Apple-silicon prerelease is
`https://github.com/NavilanSanthanakrishnan/cosmos-term/releases/tag/v0.1.0-alpha.2`.

Cosmos Term is a native WezTerm fork, not a wrapper. It retains terminal tabs,
splits, rendering, Lua configuration, and mux behavior while adding the left
explorer and using independent application/runtime identities.

## 2026-07-20 release candidate

- File preview now supports W/S one-line vertical scrolling,
  Shift+W/Shift+S five-line jumps, A/D one-column horizontal scrolling, and
  Shift+A/Shift+D eight-column jumps. Opening a file releases Explorer focus
  so the document receives these keys.
- Real tmux prefixes are buffered before file-workspace handling. Prefix+w and
  prefix+s therefore remain tmux commands, while unprefixed W/S/A/D navigate
  the owning file preview.
- The bundled renderer is capped at 30 FPS. Codex prompt inspection records
  only an output timestamp and waits for 200 ms of quiet before foreground
  process lookup and parsing, avoiding per-output work during GLX streaming.
- `featurerequest/codex-preferred-prompt-auto-selection.md` is implemented.
  It defaults to observe mode, recognizes only three complete whitelisted
  Codex selection prompts, and supports native and exact targeted tmux panes.
  Command+Option+P cycles modes; Command+Option+Escape turns it off
  immediately. The footer shows mode and successful-choice count.
- The tmux action path revalidates exact command, pane mode, prompt,
  fingerprint, and digit on the context worker. An atomic gate defaults to
  disabled and is checked immediately before send. A process-wide reservation
  prevents two Cosmos windows from acting on the same prompt.
- Audit data is owner-only JSONL with minimal metadata and no terminal text,
  prompt text, command, path, response, or conversation content.
- The dedicated-socket prompt integration test sends only the preferred digit
  to an exact disposable `codex` pane. The full workspace suite has 28 passing
  tests; GUI/CLI/mux checks, release packaging, strict signature verification,
  plist validation, and `git diff --check` pass.
- The final package and only installed app are
  `dist/Cosmos Term.app` and `/Applications/Cosmos Term.app`, bundle ID
  `com.navilan.cosmos-term`, short version
  `20260720-111529-498cf07e`, and bundle version `20260720111614`. Their
  `cosmos-term-gui` SHA-256 is
  `ba11a49ac770796085b74680f93db13fc314fcefe288e2121d195a12d35978b1`.
  The package identifies source commit `498cf07e9`. Live installed Cosmos PID
  40964 and default tmux client `/dev/ttys000 $3 %10` remained protected and
  unchanged during atomic replacement; the running process retains its prior
  in-memory executable until the user's next normal relaunch.
- A background isolated launch with unique data/runtime roots averaged 0.395%
  CPU over 20 steady-state samples. `vmmap` reported 85.8 MiB resident and
  `footprint` recorded a 224.7 MiB peak. Its only child was `/bin/sleep`, the
  worker sample was blocked in channel/`kevent`/Mach waits, and cleanup left
  live PID 40964 plus the default tmux client unchanged.
- Future Codex processes now use the preserved OpenAI-signed 0.144.6 binary
  (`TeamIdentifier=2DC432GLL2`). The prior ad-hoc always-wait binary remains a
  byte-for-byte private backup at
  `~/.local/state/tmux-manager/codex-always-wait/codex-0.144.6-always-wait-adhoc`;
  existing Codex processes were not stopped.
- The official ChatGPT GUI marketplace Computer Use plugin is installed as
  `1.0.1000387`, and fresh signed Codex successfully authenticated through its
  trusted `node_repl` wrapper. The updated service is `26.710.1000387`.
  Official per-app approval is now retained for Calculator, Safari, and the
  installed Cosmos Term. Calculator passed a UI-driven `2 + 2 = 4` acceptance
  test; Safari returned a read-only Start Page state without navigation or
  typing; Chrome remained frontmost throughout. The protected installed
  Cosmos PID 40964 remained running and unaltered. Cosmos state capture timed
  out after approval, so no current official screenshot file was produced and
  the Cosmos-specific capture remains an open acceptance item. No AppleScript,
  accessibility scripting, CGEvent, browser automation, custom keyboard tool,
  or focus restoration was used. The official API has no explicit
  never-activate flag.
- GitHub project workflows and Dependabot schedules are removed. Validation
  and dependency review are local; publishing must not invoke or add GitHub
  Actions.
- Public prerelease `v0.1.0-alpha.2` targets
  `d0f928bc27f07db97bd219f212cb484b784bd407` at
  `https://github.com/NavilanSanthanakrishnan/cosmos-term/releases/tag/v0.1.0-alpha.2`.
  Its 52,396,429-byte `Cosmos-Term-macos-arm64.zip` re-downloaded with
  SHA-256
  `d2aa1ce53a46cd9c05f1cef9f1ab09d5d7fbf789cb513cc03b9ef4979dd36ee4`;
  checksum and `unzip -t` verification passed, and GitHub reports both assets
  uploaded.
- The first feature push briefly enqueued two GitHub Dependabot dynamic checks
  even though Actions was disabled. One completed as failed before cancellation
  reached it and the other was canceled. Neither was Cosmos CI or Codex work.
  After deleting `.github/dependabot.yml`, the final release-notes head had zero
  workflow runs.

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
- `<prefix> 0` is an explicit global Explorer keyboard region from any inner
  tmux pane, including while the file workspace is hidden or owned elsewhere.
  W/S move one row, Shift+W/Shift+S move five, A/D and arrows collapse or
  expand, Return activates the selected row, and Escape exits. Cosmos buffers
  the prefix until it sees the command: 0 is consumed entirely, while every
  other prefix command leaves navigation, replays the exact prefix, and
  continues through tmux. Positional prefix+1/2 therefore remain reliable
  even when a command selects the pane that is already active.
- Physical macOS Shift+W/S input is accepted both with an explicit Shift flag
  and in the uppercase-without-Shift form produced by the text-input pass.
  An isolated packaged build verified `.cargo` to `base91` as a five-row
  Shift+S jump, the reverse Shift+W jump, and Return opening
  `docs/cli/ls-fonts.md` inside only the right tmux pane. The final captures
  are `/tmp/cosmos-verified-captures/shift-s-five-rows.png`
  (`d68ad2df238a8520ccd33339027b545e4144c32dba8a992a1dfefd9fcc603b87`),
  `enter-opened-markdown.png`
  (`3c3ba8c50df01045e05a209f08be9f2442258f8790f3c69286f80555ed0e8874`),
  and `pane-owner-keyboard-transparent.png`
  (`17e72d43b92d38fd0da649ce13e5c65edc8ca6294875f89cb2b410d6e8772044`).
  The disposable GUI and dedicated tmux server were removed; live Cosmos PID
  664 and the default tmux client `/dev/ttys000 0 %0` were unchanged.
- The global three-region mapping was verified with the user's actual
  `S-BSpace` prefix and positional 1/2 helper on a dedicated two-pane server.
  From pane 2, prefix+0 entered Explorer navigation and W/Shift+S moved the
  selection; prefix+1 selected the workspace owner, exited navigation, and
  sent the next `s` to pane 1. The same-pane `2 → 0 → 2` sequence also exited
  navigation and sent the next `s` to pane 2. With the file workspace hidden,
  prefix+0 still entered navigation from pane 2. Captures are
  `/tmp/cosmos-verified-captures/global-zero-from-pane2.png`
  (`5b3ac37bd607dc69e855e9c7eec193e9083539557002c5e4eaecc47e1e584dc3`),
  `prefix1-exits-navigation.png`
  (`26da768f1e500019c58802c68a8d255dbebed6f0ea270547c36b96b998af262e`),
  and `global-zero-workspace-hidden.png`
  (`9bdb55de65beaed10e5534295366404c85bf347491e564a33746f82cf55f138b`).
  The disposable app/server were removed; live Cosmos PID 34032 and default
  tmux client `/dev/ttys000 2 %4` were unchanged.
- The global Explorer-focus fix is public on `main` at code commit
  `707409de70818cb806650e2cff4b907d38927f93`. The commit uses `[skip ci]`;
  no pull request was opened and no GitHub workflow was invoked or rerun. The
  signed bundle installed in place at `/Applications/Cosmos Term.app` is
  version `20260719-124632-707409de`, and its `cosmos-term-gui` SHA-256 is
  `b76212854af10a3eb71ca5547279e7be4c432597162049347130f95d81e02a34`.
  Installation preserved live PID 34032 and `/dev/ttys000 2 %4`; the running
  process retains its old in-memory code until the user's next normal relaunch.
- GitHub Actions is disabled for the public repository at the repository
  permission level. Publishing the repository briefly caused GitHub's existing
  Dependabot configuration to enqueue two automatic dynamic checks; both
  completed without running project CI or consuming Codex usage.
- The horizontal-split regression was reproduced on a dedicated tmux 3.7b
  socket after a multi-column `ls`. Cosmos and `stty` both reported the exact
  100-column client split into 50/49-column panes, and the rendered divider
  matched that geometry. The rearranged historical `ls` text is tmux's own
  resize reflow; Cosmos adds no second width offset and new output uses the
  correct pane width.
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
- `Command+S` toggles only the focused native or inner tmux pane between its
  terminal and file workspace. Native pane bounds come from WezTerm; inner
  tmux bounds come from snapshots of every pane's ID and cell geometry. Every
  inactive pane remains rendered and live. A new pane context opens a blank
  `FILE WORKSPACE` with no selected file or search field; the user must choose
  a visible file from the left Explorer.
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
  suite now has 24 passing tests, including tmux geometry parsing,
  out-of-root rejection, atomic save cleanup, and external-revision conflict.
  Full GUI/CLI/mux checks,
  release packaging, strict signature verification, plist validation, and
  `git diff --check` pass. The repository-wide format check still reports only
  the pre-existing `window/src/os/macos/app.rs` difference.
- The final pane-scoped release was exercised against a two-pane tmux session
  on a dedicated socket. Left and right selection each produced an opaque file
  workspace inside only the active tmux pane while the other pane stayed
  visible. Captures are
  `/tmp/cosmos-file-workspace-capture/tmux-pane-final2-left.png`
  (`bcb87493ea7973920ab3c723e39c818f6d910c7db0b4562d27eae5124d8e1bf7`)
  and `tmux-pane-final2-right.png`
  (`c5d950301de4fc76bc6a5a12d64c5b7f4a7bdf0cee343fe4a3c1b7752cafc137`).
  The dedicated server was stopped, and default tmux clients and installed
  WezTerm processes were unchanged.
- The exact signed binary installed at `/Applications/Cosmos Term.app` has
  SHA-256
  `508a55d03eabffc0de86a9ff8f0784be01a95f5ad2b0fd342e9595e05fd17ea9`.
  Its short bundle version is `20260718-235004-74aafb0c`, which identifies
  public merge `74aafb0c7f30169b56901da791b50b53e8a64454`.
  No installed Cosmos process was running during replacement. The merged
  application is now open as PID 186, and default tmux still has no clients.
  The installed-window capture is
  `/tmp/cosmos-pane-owner-capture/07-installed-merged-app.png`
  (`ad138af2cdc17c7a55aa8b2743c91e5f33af60ad747cb80a07c395555e408d9d`).
  All old release backups, disposable test bundles, and the generated `dist`
  bundle were deleted;
  `/Applications/Cosmos Term.app` is the only runnable Cosmos Term bundle
  outside the required source template.
- Tmux file-preview mode is now keyboard-transparent after explicit Cosmos
  shortcuts are handled. The active inner `pane_id` remains tracked
  separately from geometry and CWD, and edit mode retains raw configured
  `prefix`/`prefix2` passthrough because it deliberately owns text input.
  A dedicated two-pane server using the user's `S-BSpace` prefix verified
  `prefix+d` and `prefix+a` pane movement, a multi-character `:` command
  prompt, copy-mode entry and exit, new/previous-window commands, a repeated
  key-table command, and direct `bind-key -n` commands while preview remained
  visible. Explorer keyboard focus is also bypassed during tmux preview, so
  selecting a file cannot re-capture tmux keys. The dedicated server and
  disposable app were removed;
  default tmux still had only `/dev/ttys031 16 %50 /Users/navilan`, and live
  Cosmos PID 71709 was unchanged. The final capture is
  `/tmp/cosmos-tmux-transparent-capture/file-preview-tmux-transparent-front.png`
  (`a7d9f562dd2ac569ed0dc0378dbdec92442c90ee33a850465f4dff71d7671341`).
  The workspace suite remains at 24 passing tests; full GUI/CLI/mux checks,
  release packaging, signature verification, plist validation, and
  `git diff --check` pass.
- The file workspace is now owned by the native and inner tmux pane where
  `Command+S` opened it. The user's positional `prefix+1`/`prefix+2` commands
  change focus without making the workspace jump sides; `Command+S` on the
  other pane deliberately transfers it. A real `swap-pane` follows the owner
  ID, resize follows refreshed geometry, deleting the owner restores the
  terminal, switching tmux windows hides/restores the surface with its owner,
  and an inactive owner in Edit mode does not steal input from the focused
  pane. The isolated captures are
  `/tmp/cosmos-pane-owner-capture/02-owner-left-active-right.png`
  (`91f976211189f891922c7a715cac10192d761e697903dc011a1a70765cc1df84`),
  `04-real-swap-follows-owner-id.png`
  (`e48bbfbe3e7c10d79ed7b44f179d6e0fcd2db511bc3961bb963d2ac7d69a68c9`),
  `05-owner-resized.png`
  (`bf645d246190b4738e856f88498c1fc8154d9b7f3341bae69b913a6571b8c972`),
  and `06-owner-removed.png`
  (`6c78428aa412d8a3f1a2eab42d2294c5813d03603e7e07c9e0f997b359c03e1f`).
  A second dedicated server verified window persistence with
  `/tmp/cosmos-pane-window-capture/01-owner-window-specific.png`
  (`d0ff82bf996b9fed0ea55538ad63bac61d9af16c86245e7638a8bdb78f62f686`),
  `02-away-window-hidden.png`
  (`d7ffce157307b54aa8f005c4dad76758c764246c59144ef39d718ed9d6b0d704`),
  and `03-owner-window-restored.png`
  (`001ba780a72601b14c6b5a8f762b1a696487754952bc39bf594c36003ac36a8d`).
  The dedicated app/server/root were removed, and default tmux still had no
  clients.

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
