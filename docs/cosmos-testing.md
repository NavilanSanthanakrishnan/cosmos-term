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
parsing, directory sorting/filtering, persistence, structured Codex usage
parsing, reset selection, and exact executable-name process detection.

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
env \
  WEZTERM_CONFIG_FILE=/tmp/must-not-load.lua \
  WEZTERM_UNIX_SOCKET=/tmp/must-not-connect.sock \
  "dist/Cosmos Term.app/Contents/MacOS/cosmos-term" show-keys
```

The command must load the bundled Cosmos config, list `Command+Shift+E` as
`Nop`, list `Command+W` as a custom event callback, and contain no key
assignment that hides, locks, or changes Explorer scope. It must not require or
connect to WezTerm.

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
11. In a Git worktree, modify a visible file and confirm a right-aligned `M`
    appears and refreshes without blocking terminal input.
12. Press `Command+W` and confirm the in-app `COSMOS TERM CLOSE LOCK` screen
    appears with no `tmux Manager` dialog or notification. Type a disposable
    value and confirm only bullets render, then press Escape and confirm the
    tab and its processes remain intact. In a disposable test tab backed by a
    dedicated tmux socket and temporary close-lock credential, enter the
    correct passphrase and confirm the workspace is saved before the tab and
    all of its panes close.
13. Capture native window screenshots on 72 DPI and 144 DPI displays. Confirm
    the Explorer remains 520 logical pixels wide with a 35 px title, 22 px
    rows, 13/15 px text, and exact `#252526` background and `#37373D` inactive
    selection pixels on the 72 DPI reference display.
14. While the terminal repaints under rapid input, confirm cached expanded
    directories are not re-read. Expand an uncached folder and confirm its
    worker result appears on the 50 ms service-response cycle; watcher and
    periodic refreshes must remain the only background rescan paths.
15. Confirm the bottom bar reports the current Codex usage window, nearest
    reset, and active loop count. Start one disposable `codex` executable and
    confirm the count increments within two seconds; stop it and confirm the
    count decrements without restarting Cosmos.
16. Inspect Cosmos child processes while the footer updates. Confirm there is
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
- `tmux -S "$sock" select-pane -t <second-pane>` resolves `/path/two`
- the terminal remains responsive

Stop only the test Cosmos process and run
`tmux -S "$sock" kill-server`. Compare the default tmux client list with the
recorded baseline.

## Final isolation audit

- Installed WezTerm PID is still running if it was running before the test.
- Default tmux sessions and clients are unchanged.
- Cosmos sockets exist only below its runtime directory.
- No `dist/`, logs, runtime JSON from the home directory, or secrets are
  staged for Git.
