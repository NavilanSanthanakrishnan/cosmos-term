# Cosmos Term Architecture

## Product boundary

Cosmos Term is a direct WezTerm fork with a native left-side workspace
explorer. The terminal engine remains WezTerm: PTYs, tabs, splits, mux domains,
rendering, fonts, input handling, and Lua configuration continue to use the
existing implementation. The explorer is composed into the same native window
and render pipeline.

V1 deliberately avoids becoming an editor, an Electron wrapper, or a separate
process embedded beside the terminal.

## Components

### `cosmos-workspace`

This crate owns UI-independent workspace behavior:

- serializable explorer state and atomic persistence
- workspace root add/remove/rename/reorder operations
- Follow, Project Follow, and Locked expansion semantics
- project-root discovery
- lazy direct-directory listings and stable sorting
- row generation for expanded directories
- pane-context resolution for native panes and tmux
- filesystem change notification

Three independent worker threads serve directory reads, pane-context requests,
and filesystem watches. Responses return through a single non-blocking channel
consumed by the window. The render thread never recursively scans a workspace.

Only expanded directories are watched, and watches are non-recursive. A
periodic refresh provides a fallback if a platform watcher misses an event.
Listings are capped at 5,000 visible entries per directory and report
truncation inline.

### `wezterm-gui/src/termwindow/cosmos.rs`

The native window adapter owns:

- persisted `ExplorerUi` state
- active-pane polling and context application
- Code OSS-inspired Activity Bar, primary-sidebar title, section header,
  explicit mode control, proportional row text, vector controls, list
  highlights, and inline errors
- mouse hit targets, scrolling, divider drag, and row activation
- keyboard navigation and root prompts
- spawning selected directories into tabs or splits

The explorer width becomes a left origin offset for tab bars, panes, split
backgrounds, and terminal rendering. Existing WezTerm widgets continue to use
their normal layout inside the remaining viewport.

### Narrow upstream integration

The surrounding changes are intentionally limited to:

- constructing and polling the explorer from `TermWindow`
- offsetting render/layout coordinates
- routing explorer mouse and keyboard events, including explicit focus return
  when a terminal pane is clicked
- defining menu/command-palette actions while consuming the retired sidebar
  toggle chord
- standalone app/config/runtime identity
- modern compiler and macOS SDK compatibility

## Pane following

For a native pane, WezTerm's reported CWD is used. The explorer resolves the
longest matching saved workspace root, discovers a containing Git project,
and applies the selected follow mode. The visible root is transient and
independent from saved multi-root state, which prevents parent or sibling
roots from leaking into a folder-scoped view.

For tmux, the native terminal pane still reports the outer shell's CWD, so the
foreground process and TTY are also inspected. When the foreground process is
`tmux`, Cosmos Term runs:

```sh
tmux display-message -p -t <client-tty> "#{pane_current_path}"
```

The process inherits the GUI's `TMUX` environment and therefore queries the
same server as the attached client. Cosmos invokes the foreground tmux
process's full executable path when available, so a Finder-launched GUI does
not depend on Homebrew being present in its reduced `PATH`. If the query
fails, the explorer keeps the terminal usable, falls back to reported or
last-known context, and displays the error in the sidebar.

Context requests run approximately four times per second, independently from
directory loading. This makes native focus and tmux pane changes visible
without shell hooks. OSC 7 remains useful because it improves the CWD that
WezTerm reports for native and remote-aware shells.

## Follow modes

- **Follow** makes the focused pane's CWD the sole visible explorer root. Only
  that folder's contents are shown.
- **Project Follow** makes the detected Git project the sole visible root and
  reveals the focused CWD within it.
- **Locked** holds the current visible root and expansion state while terminal
  CWD changes continue.
- **Reveal Active** explicitly switches a Locked view to the focused CWD.

## Persistence

Explorer state is stored atomically as JSON at:

```text
~/Library/Application Support/cosmos-term/workspace-state.json
```

The file contains sidebar width, roots and labels, expanded directories,
follow mode, and hidden-file preference. A legacy visibility field is retained
for state compatibility but ignored because the explorer is now a permanent
workbench region. Cached listings and pane context are intentionally transient.

## Isolation

Cosmos Term uses its own bundle ID, config names, data/cache directories,
runtime socket variables, executable variables, logs, and companion binary
names. It ignores WezTerm's config and socket variables so both applications
can run simultaneously without sharing GUI or mux sessions.

At bootstrap, inherited WezTerm config/protocol variables are removed. A
GUI-level `TMUX` value may be retained so the pane resolver can query the
server from which Cosmos was intentionally launched, but `TMUX`, `TMUX_PANE`,
and all parent WezTerm protocol variables are removed from each newly spawned
terminal command. This prevents a fresh Cosmos shell from masquerading as an
attached pane in the parent terminal.

The upstream updater is disabled because Cosmos Term artifacts are not WezTerm
artifacts. Updating the fork is an explicit source/release process.

## Compatibility notes

The baseline predates the current Rust compiler and macOS SDK. Null-safe
FreeType handling and the macOS full-screen constant adjustment are narrow
backports from later upstream behavior. The package-specific `glium`
optimization override is required to keep the baseline's default OpenGL
renderer correct on the current LLVM toolchain.
