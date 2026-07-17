# Cosmos Term

<img src="assets/icon/cosmos-term.svg" width="112" alt="Cosmos Term icon" align="left">

Cosmos Term is a standalone, native fork of WezTerm with a VS Code-style
filesystem explorer integrated into the left side of every terminal window.
It retains WezTerm's terminal engine, tabs, splits, rendering, configuration
model, and multiplexer support while keeping its application identity and
runtime state separate from an installed WezTerm.

The current V1 is a macOS application based on WezTerm commit
`5046fc225992db6ba2ef8812743fadfdfe4b184a`, matching the WezTerm version that
was installed when this fork was created.

## V1

- Native, persistent, resizable explorer beside terminal tabs and splits
- Code OSS Dark Modern Explorer styling with a 35 px title, compact 22 px
  tree rows, proportional UI text, Seti-style colored file icons, and native
  chevrons and selection states
- Roomier defaults: a 420 px explorer, 14 pt text, and a 100 × 32 terminal
  viewport
- Right-aligned Git decorations for modified, added, deleted, renamed,
  untracked, and conflicted files, resolved off the render thread
- Folder-scoped Follow mode: the focused pane's working directory is the only
  visible root, so parent and sibling folders are not shown
- tmux-aware reveal based on the selected tmux pane, including pane changes
- Follow, Project Follow, and Locked modes
- Multiple named, reorderable workspace roots
- Lazy directory loading and live non-recursive filesystem watching
- Add, remove, rename, expand, collapse, and reorder root interactions
- New terminal tab or split from a selected directory
- Persistent sidebar width, roots, expansion, follow mode, and hidden-file
  preference
- Non-destructive inline errors for inaccessible or invalid paths

Cosmos Term is the terminal application itself—not a wrapper around WezTerm
and not an editor embedding a terminal.

## Explorer controls

The explorer is always visible, and the legacy `Command+Shift+E` chord is
intentionally inert. The header matches VS Code's compact `EXPLORER` title and
single ellipsis action. Clicking the ellipsis cycles the follow mode; it turns
blue while the view is Locked.

Dotfiles are shown by default to match the reference project view, except for
repository/runtime metadata such as `.git` and `.DS_Store`.

Click a row to select and expand/collapse it. Double-click a directory to open
it in a new tab. Drag the divider to resize the sidebar.

When the explorer has keyboard focus:

| Key | Action |
| --- | --- |
| `↑` / `↓` | Move selection |
| `←` / `→` | Collapse/parent or expand/child |
| `Return` | Expand or collapse |
| `Command+Return` | Open directory in a new tab |
| `Shift+Return` | Open directory in a split |
| `A` | Add a root |
| `F` / `P` | Select Follow or Project Follow |
| `L` | Toggle between Follow and Locked |
| `R` | Reveal the active pane |
| `.` | Toggle hidden files |
| `F2` | Rename the selected root label |
| `Delete` | Remove the selected root without deleting files |
| `Escape` | Return focus to the terminal |

Explorer actions are also available through the command palette and View
menu. Clicking a terminal pane immediately returns keyboard focus to the
terminal.

`Command+W` closes the current tab immediately. `Command+Q` keeps the
protected whole-application autosave flow.

## Isolation from WezTerm

| Concern | Cosmos Term |
| --- | --- |
| macOS bundle ID | `com.navilan.cosmos-term` |
| App | `/Applications/Cosmos Term.app` |
| User config | `~/.config/cosmos-term/cosmos.lua` or `~/.cosmos-term.lua` |
| Bundled fallback config | `Cosmos Term.app/Contents/Resources/cosmos.lua` |
| Persistent data | `~/Library/Application Support/cosmos-term` |
| Runtime sockets and logs | `~/Library/Caches/cosmos-term/runtime` |
| Protocol environment | `COSMOS_TERM_UNIX_SOCKET`, `COSMOS_TERM_PANE` |
| Config environment | `COSMOS_TERM_CONFIG_FILE`, `COSMOS_TERM_CONFIG_DIR` |
| Child terminal identity | `TERM_PROGRAM=CosmosTerm` |

Cosmos Term does not read `~/.wezterm.lua`, does not use
`WEZTERM_UNIX_SOCKET`, and cannot accidentally direct its CLI at a running
WezTerm GUI. The bundled config initially mirrors the personal WezTerm
behavior that existed when the fork was created. Parent WezTerm protocol
variables and stale tmux attachment variables are removed from new Cosmos
terminal shells.

## Build and package on macOS

Prerequisites are the same as the upstream WezTerm macOS build plus a current
Rust toolchain and Xcode Command Line Tools.

```sh
git submodule update --init --recursive
cargo build --release -p wezterm-gui -p wezterm -p wezterm-mux-server
ci/package-cosmos-macos.sh
```

The packaging script creates and ad-hoc signs `dist/Cosmos Term.app`. To
install a local build, quit any running Cosmos Term instance and copy that
bundle to `/Applications/Cosmos Term.app`.

For development checks:

```sh
cargo test -p cosmos-workspace
cargo check -p wezterm-gui -p wezterm -p wezterm-mux-server
```

See [Cosmos architecture](docs/cosmos-architecture.md) and
[testing](docs/cosmos-testing.md) for implementation and verification details.
The original product direction is retained in
[the product vision](navilan-terminal-workspace-product-vision.docx).

## Upstream and license

Cosmos Term is derived from [WezTerm](https://github.com/wez/wezterm), created
by Wez Furlong and contributors. The original copyright, MIT license, bundled
font licenses, and upstream history are retained. Cosmos-specific work is also
distributed under the repository's MIT license.

The explorer's layout metrics, Dark Modern palette, and Seti icon conventions
are based on the MIT-licensed
[Microsoft Code - OSS](https://github.com/microsoft/vscode) explorer,
list/tree, pane-header, default-theme, and Seti-theme sources. Cosmos Term uses
its own native renderer; it does not bundle or launch VS Code.
