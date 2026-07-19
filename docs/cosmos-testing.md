# Cosmos Term Testing

## Automated checks

Run from the repository root:

```sh
cargo test -p cosmos-workspace
cargo check -p wezterm-gui -p wezterm -p wezterm-mux-server
git diff --check
```

The workspace tests cover legacy follow-state compatibility, root matching and
deduplication, folder-scoped row isolation, layout migration, Git porcelain
parsing, directory sorting/filtering, persistence, workspace-confined
load/save, atomic-save cleanup, external-change conflict
detection, structured Codex usage parsing, reset selection, exact
executable-name process detection, CPU tick delta calculation, RAM label
formatting, and native macOS capacity reads.

## Release bundle

```sh
cargo build --release -p wezterm-gui -p wezterm -p wezterm-mux-server
ci/package-cosmos-macos.sh
codesign --verify --deep --strict "dist/Cosmos Term.app"
plutil -lint "dist/Cosmos Term.app/Contents/Info.plist"
```

Confirm that the bundle contains `cosmos-term-gui`, `cosmos-term`, and
`cosmos-term-mux-server`, and that the plist reports
`com.navilan.cosmos-term`.

## Performance regression

Measure a fresh Finder-style launch with the same CWD, window geometry,
display, and settling time used for a fresh WezTerm comparison:

```sh
footprint <pid>
vmmap -summary <pid>
top -l 30 -s 1 -pid <pid> -stats pid,cpu,mem,command
sample <pid> 12 1 -file /tmp/cosmos-cpu-sample.txt
```

On the 2026-07-17 reference Mac, the pre-optimization Cosmos build reached a
954–956 MiB physical-footprint peak and averaged 1.21% idle CPU. The optimized
installed bundle measured 81.8 MiB current footprint, 237.6 MiB peak, and
0.48% average idle CPU across 30 steady-state samples. A matched fresh WezTerm
launch measured 56.8 MiB current, 137.1 MiB peak, and 0.54% average idle CPU.
Treat these as machine-specific reference values: regressions should be judged
against a matched launch rather than a universal absolute threshold.

The 2026-07-18 capacity-footer build averaged 0.420% idle CPU across 30
steady-state samples and settled at 87.4 MiB current footprint with a 245.4
MiB peak. Its same-session pre-feature run measured 80 MiB current and 242 MiB
peak; launch-to-launch renderer variation is larger than the capacity
snapshot itself. The final design samples no faster than once per ten seconds
and skips invalidation when raw page changes do not alter the visible label.

The high-water check must not show hundreds of MiB retained in empty
`MALLOC_SMALL` regions. A native sample should show the Cosmos worker threads
blocked on their channels between requests, with no persistent font-catalog
enumeration, directory scan from paint, or external status helper.

The old `glium` dependency must retain its package-specific release
`opt-level = 0` for compatibility. The bundled Cosmos config intentionally
selects WebGPU because its sRGB surface preserves VS Code theme values on both
standard and wide-gamut macOS displays. Validate that configured default at
both 72 and 144 DPI.

On a mixed-DPI Mac, launch from both the standard and Retina screens. After
Cocoa completes placement, the window must settle to the configured 100 × 32
terminal geometry plus the 520 logical-pixel Explorer and 22 logical-pixel
status bar. Type into the terminal and move the window between screens; it
must not re-expand, retain stale WebGPU quads, or change the Explorer's
apparent scale.

## Config and protocol isolation

Launch or query Cosmos Term with deliberately hostile WezTerm variables:

```sh
test_root="$(mktemp -d /tmp/cosmos-config-test.XXXXXX)"
env \
  COSMOS_TERM_CLOSE_LOCK_FILE="$test_root/missing-close-lock.json" \
  COSMOS_TERM_DATA_DIR="$test_root/data" \
  COSMOS_TERM_RUNTIME_DIR="$test_root/runtime" \
  WEZTERM_CONFIG_FILE=/tmp/must-not-load.lua \
  WEZTERM_UNIX_SOCKET=/tmp/must-not-connect.sock \
  "dist/Cosmos Term.app/Contents/MacOS/cosmos-term" \
  --config-file "$PWD/dist/Cosmos Term.app/Contents/Resources/cosmos.lua" \
  show-keys
```

The command must load the bundled Cosmos config, list `Command+Shift+E` as
`Nop`, list `Command+W` as a confirmed `CloseCurrentTab`, and contain no key
assignment that hides, locks, or changes Explorer scope. It must not require
tmux-manager or connect to WezTerm. The explicit config path ensures an
existing developer config cannot mask the bundled release config during this
probe.

Run a second `show-keys` query with the actual protected-close environment, if
one is configured. In that mode `Command+W` and `Command+Q` must be custom
event callbacks backed by the concealed in-app prompt. Do not test the success
path against a live tab: use a temporary PBKDF2 credential, disposable state,
and a dedicated tmux socket.

When Cosmos is launched from inside an existing WezTerm/tmux pane, inspect the
new Cosmos shell:

```sh
env | grep -E '^(WEZTERM|TMUX)'
```

It must print no inherited WezTerm protocol/config values and no stale
`TMUX`/`TMUX_PANE` attachment.

## Live native-pane matrix

Use only panes created in Cosmos Term.

1. Launch with a known repository CWD and confirm that folder is the sole
   visible root. No saved historical root may flash before it resolves.
2. Run `cd` into a nested directory and confirm it becomes the sole visible
   root without parent or sibling folders.
3. Create a split with a different CWD. Switch focus in both directions and
   confirm the highlight and status follow the focused pane.
4. Create and remove a directory under an expanded root. Confirm it appears
   and disappears without restarting.
5. Resize the divider, change expansion state, restart, and confirm the width
   and reachable expansion state persist while the new pane CWD remains the
   sole root.
6. Open a selected directory in a tab and a split.
7. Use an invalid or inaccessible root and confirm an inline error without a
   crash or blocked terminal.
8. Focus an Explorer row and press `L`. Confirm literal `l` reaches the shell
   and the Explorer remains visible; there is no lock or hide binding.
9. Click the terminal, type a command containing `.` and press Return. Confirm
    the command reaches the shell and no explorer action runs.
10. Press `Command+Shift+E` and confirm the sidebar remains visible and no `E`
    reaches the shell.
11. Press `Command+S` and confirm an empty file workspace overlays only the
    focused native pane. Confirm every inactive split remains visible and no
    file or search field is selected. Click a Markdown file in the left
    Explorer and verify formatted headings, lists, quotes, code, and link
    destinations. Press `Command+S` again and confirm the original shell
    process, screen, and scrollback return.
12. Click a visible UTF-8 file, enter edit mode with `Command+E`, make a
    disposable change, and save with `Command+Return`. Confirm permissions
    are unchanged and no `.cosmos-save-*` file remains. Modify a second
    disposable copy externally after opening it and confirm Cosmos refuses to
    overwrite the newer revision.
13. Leave an edit dirty and confirm `Command+W` and `Command+Q` are blocked.
    Save it, or deliberately discard it with `Command+Shift+D`, then confirm
    normal protected-close behavior resumes.
14. In a Git worktree, modify a visible file and confirm a right-aligned `M`
    appears and refreshes without blocking terminal input.
15. Press `Command+W` and confirm the in-app `COSMOS TERM CLOSE LOCK` screen
    appears with no `tmux Manager` dialog or notification. Type a disposable
    value and confirm only bullets render, then press Escape and confirm the
    tab and its processes remain intact. In a disposable test tab backed by a
    dedicated tmux socket and temporary close-lock credential, enter the
    correct passphrase and confirm the workspace is saved before the tab and
    all of its panes close.
16. Capture native window screenshots on 72 DPI and 144 DPI displays. Confirm
    the Explorer remains 520 logical pixels wide with a 35 px title, 22 px
    rows, 13/15 px text, and exact `#252526` background and `#37373D` inactive
    selection pixels on the 72 DPI reference display.
17. While the terminal repaints under rapid input, confirm cached expanded
    directories are not re-read. Expand an uncached folder and confirm its
    worker result appears on the 50 ms service-response cycle; watcher and
    periodic refreshes must remain the only background rescan paths.
18. Confirm the bottom bar reports the current Codex usage window, nearest
    reset, and active loop count. Start one disposable `codex` executable and
    confirm the count increments within two seconds; stop it and confirm the
    count decrements without restarting Cosmos.
19. Compare the footer's system-wide CPU and occupied/total RAM values with
    Activity Monitor while idle and under a short disposable load. Confirm the
    rolling view updates within twelve seconds and remains legible at the
    minimum window width.
20. Inspect Cosmos child processes while the footer updates. Confirm there is
    no status helper, shell loop, daemon, or repeated `ps`/`pgrep` process.
    Updating the active rollout should not trigger a full session-tree walk;
    broad discovery is rate-limited to once per 15 seconds.

## Isolated tmux matrix

Record the user's default tmux clients first:

```sh
tmux list-clients -F '#{client_tty} #{session_id} #{pane_id} #{pane_current_path}'
```

Create a dedicated server. Every mutating tmux command must include `-S`:

```sh
sock=/tmp/cosmos-term-test-$$.sock
tmux -S "$sock" new-session -d -s cosmos-test -c /path/one
tmux -S "$sock" split-window -d -t cosmos-test -c /path/two
server_pid=$(tmux -S "$sock" display-message -p '#{pid}')
```

Launch a separate Cosmos Term GUI process with
`TMUX="$sock,$server_pid,0"`, then attach its test terminal pane using
`TMUX= tmux -S "$sock" attach-session -t cosmos-test`.

Verify:

- sidebar source says `tmux`
- initial selected pane resolves `/path/one`
- `Command+S` overlays only the first tmux pane rectangle; the second pane
  remains visible and its terminal content does not bleed through the overlay
- `tmux -S "$sock" select-pane -t <second-pane>` resolves `/path/two`
- while the first pane's workspace remains visible, use positional selection
  such as the configured `<prefix> 2` and `<prefix> 1`; confirm focus changes
  but the workspace remains attached to the first pane instead of swapping
  sides
- press `Command+S` while the second pane is active; confirm this explicit
  action moves the single workspace to the second pane and leaves the first
  pane visible
- while the file workspace remains visible, use the server's configured tmux
  prefix plus pane-navigation command in both directions; confirm tmux changes
  the active pane, the overlay stays with its owner, and neither command key
  appears in either shell
- enter workspace edit mode, navigate to the other pane, and trigger a
  disposable direct binding; confirm the inactive editor does not capture the
  newly focused pane's input
- run `tmux -S "$sock" swap-pane` and confirm the workspace moves with its
  owning pane ID; resize that pane and confirm the overlay follows its new
  geometry without resetting
- open the tmux command prompt with `<prefix> :`, type a multi-character
  command such as `select-pane -R`, and press Return; confirm it executes while
  the file preview remains visible
- enter copy mode with `<prefix> [`, confirm `#{pane_in_mode}` is `1`, then
  press the configured copy-mode exit key and confirm it returns to `0`
- create and revisit a tmux window with the normal window bindings; confirm
  the workspace is hidden while its owning window is away and restored in the
  same pane when that window returns
- add a disposable `bind-key -n` binding only to the dedicated server and
  confirm it executes without a prefix while preview remains visible
- click a file or the Explorer surface so the sidebar is visually focused,
  then confirm a disposable direct binding on an Explorer key such as `R`
  still reaches tmux instead of triggering an Explorer action
- from both the workspace owner and the other tmux pane, press `<prefix> 0`;
  confirm the Explorer receives the active selection, W/S move one row,
  Shift+W/Shift+S move five rows, and Return opens the selected file in the
  active pane surface. Repeat once while the file surface is hidden. Exercise
  the Shift shortcuts through physical macOS key events because the text-input
  pass can normalize them to uppercase W/S without a Shift modifier flag
- while Explorer keyboard navigation is active, run positional `<prefix> 1`
  and `<prefix> 2`; confirm each exits navigation and selects the intended
  tmux pane without moving or swapping the workspace. Confirm the next W/S key
  reaches that pane rather than remaining captured by the Explorer
- print a multi-column `ls`, create a horizontal split, and record
  `#{client_width}`, `#{pane_width}`, and `stty size` for both panes. Confirm
  the rendered divider agrees exactly with tmux's reported 50/49-style grid;
  historical `ls` columns may reflow when tmux narrows the original pane, but
  Cosmos must not add a second width offset or corrupt the new pane
- the terminal remains responsive
- as the final destructive check, kill the owning pane only on the dedicated
  server and confirm Cosmos returns to terminal mode while the remaining pane
  stays usable

Stop only the test Cosmos process and run
`tmux -S "$sock" kill-server`. Compare the default tmux client list with the
recorded baseline.

## Final isolation audit

- Installed WezTerm PID is still running if it was running before the test.
- Default tmux sessions and clients are unchanged.
- Cosmos sockets exist only below its runtime directory.
- The isolated data/runtime override directories contain no sockets belonging
  to the live installation.
- No `dist/`, logs, runtime JSON from the home directory, or secrets are
  staged for Git.
