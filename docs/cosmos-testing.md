# Cosmos Term Testing

## Automated checks

Run from the repository root:

```sh
cargo test -p cosmos-workspace
cargo check -p wezterm-gui -p wezterm -p wezterm-mux-server
git diff --check
```

The workspace tests cover follow expansion, Locked behavior, root matching and
deduplication, folder-scoped row isolation, directory sorting/filtering,
persistence, and process detection.

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
`opt-level = 0`. Test the default OpenGL renderer; a WebGPU-only success does
not verify the configured default.

## Config and protocol isolation

Launch or query Cosmos Term with deliberately hostile WezTerm variables:

```sh
env \
  WEZTERM_CONFIG_FILE=/tmp/must-not-load.lua \
  WEZTERM_UNIX_SOCKET=/tmp/must-not-connect.sock \
  "dist/Cosmos Term.app/Contents/MacOS/cosmos-term" show-keys
```

The command must load the bundled Cosmos config and list
`ToggleFileExplorer`. It must not require or connect to WezTerm.

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
   visible root.
2. Run `cd` into a nested directory and confirm it becomes the sole visible
   root without parent or sibling folders.
3. Create a split with a different CWD. Switch focus in both directions and
   confirm the highlight and status follow the focused pane.
4. Create and remove a directory under an expanded root. Confirm it appears
   and disappears without restarting.
5. Resize the divider, change expansion state, restart, and confirm the width,
   roots, expansion, and follow mode persist.
6. Select Locked, change the terminal CWD, and confirm the visible root and
   expansion state remain byte-for-byte unchanged.
7. Open a selected directory in a tab and a split.
8. Use an invalid or inaccessible root and confirm an inline error without a
   crash or blocked terminal.

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
