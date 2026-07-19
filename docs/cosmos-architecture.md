# Cosmos Term Architecture

## Product boundary

Cosmos Term is a direct WezTerm fork with a native left-side workspace
Explorer. The terminal engine remains WezTerm: PTYs, tabs, splits, mux domains,
rendering, fonts, input handling, and Lua configuration continue to use the
existing implementation. The Explorer and file workspace are composed into
the same native window and render pipeline. File mode overlays only its owning
native pane's bounds or inner tmux pane's cell geometry; it never replaces,
restarts, or detaches the underlying terminal/tmux pane.

Cosmos deliberately avoids an Electron wrapper or separate editor process.
Its file workspace is a bounded native viewer/editor for quick inspection and
small changes, not a language-server or IDE runtime.

## Components

### `cosmos-workspace`

This crate owns UI-independent workspace behavior:

- serializable explorer state and atomic persistence
- compatibility loading and migration of legacy workspace-root/follow state
- exact active-pane-directory scoping
- project-root discovery for Git decorations
- lazy direct-directory listings and stable sorting
- workspace-confined UTF-8 file loading
- revision-checked, permission-preserving atomic text saves
- row generation for expanded directories
- pane-context resolution for native panes and tmux, including active identity
  plus snapshots of every inner pane's geometry
- non-blocking Git status snapshots and porcelain parsing
- read-only Codex usage snapshots and native active-process counting
- native system-wide CPU and occupied-memory snapshots
- filesystem change notification

Five independent worker threads serve directory reads, on-demand file
workspace operations, pane-context requests, Git status requests, and
filesystem watches. Responses return through a single non-blocking channel
consumed by the window. The file worker sleeps on its channel unless the user
loads or saves a file. The render thread never reads or writes file contents,
invokes Git, or reads Codex session data. Codex and machine-capacity status
work continue to share the existing pane-context worker rather than adding a
status helper.

Only expanded directories are watched, and watches are non-recursive. A
30-second periodic refresh provides a fallback if a platform watcher misses an
event. Listings are capped at 5,000 visible entries per directory and report
truncation inline.

### `wezterm-gui/src/termwindow/cosmos.rs`

The native window adapter owns:

- persisted `ExplorerUi` state
- active-pane polling and context application
- Code OSS primary-sidebar title and exact compact tree geometry, proportional
  macOS system UI row text, the bundled Code OSS Seti font, native chevrons,
  Git decorations, list highlights, and inline errors
- logical-to-physical DPI scaling for the complete Explorer and WebGPU color
  output that preserves the reference CSS values on standard and wide-gamut
  displays
- mouse hit targets, scrolling, divider drag, and row activation
- keyboard navigation and root prompts
- spawning selected directories into tabs or splits
- switching only the selected native/tmux pane surface between the live
  terminal, an initially empty file workspace, formatted Markdown/text
  preview, and text edit mode, then retaining that pane as the surface owner
- file-workspace mouse/keyboard routing, document scrolling, line-numbered
  editing, dirty state, and native mode/path header
- a native Code OSS Dark Modern status bar that reserves 22 logical pixels
  below the Explorer and terminal

The explorer width becomes a left origin offset for tab bars, panes, split
backgrounds, and terminal rendering. The status bar similarly becomes a
bottom layout inset for terminal rows, scrollbars, and bottom-positioned tab
bars. Existing WezTerm widgets continue to use their normal layout inside the
remaining viewport.

Explorer rows are cached and regenerated only after directory, selection,
expansion, or scope changes; Git decorations are looked up from their separate
snapshot during paint. The service tick runs at 50 ms while a worker request
is pending and backs off to 500 ms while idle, so terminal painting does not
imply filesystem work or a permanent high-frequency timer.

### Narrow upstream integration

The surrounding changes are intentionally limited to:

- constructing and polling the Explorer/file workspace from `TermWindow`
- offsetting render/layout coordinates
- routing Explorer and file-workspace mouse/keyboard events, including
  explicit focus return when a terminal pane or tab is selected
- defining menu/command-palette actions while consuming the retired sidebar
  toggle chord
- resolving known registered UI fonts directly from WezTerm's built-in font
  map before falling back to the platform font locator
- standalone app/config/runtime identity
- modern compiler and macOS SDK compatibility

## Pane following

For a native pane, WezTerm's reported CWD is used as the sole visible Explorer
root. The visible root is transient and independent from serialized legacy
multi-root state, which prevents a previous root, parent, or sibling from
leaking into the current-folder view. A containing Git project may still be
discovered independently for status decorations; it never changes the visible
root.

For tmux, the native terminal pane still reports the outer shell's CWD, so the
foreground process and TTY are also inspected. When the foreground process is
`tmux`, Cosmos Term runs:

```sh
tmux display-message -p -t <client-tty> \
  "#{pane_current_path}\x1f#{pane_id}\x1f#{window_id}\x1f#{pane_left}\x1f#{pane_top}\x1f#{pane_width}\x1f#{pane_height}"
tmux list-panes -a -F \
  "#{pane_current_path}\x1f#{pane_id}\x1f#{window_id}\x1f#{pane_left}\x1f#{pane_top}\x1f#{pane_width}\x1f#{pane_height}"
```

The process inherits the GUI's `TMUX` environment and therefore queries the
same server as the attached client. Cosmos invokes the foreground tmux
process's full executable path when available, so a Finder-launched GUI does
not depend on Homebrew being present in its reduced `PATH`. If the query
fails, the explorer keeps the terminal usable, falls back to reported or
last-known context, and displays the error in the sidebar.

The active identity chooses the Explorer root. When the file workspace opens,
that native and inner pane identity becomes its owner. The corresponding cell
geometry is translated through the outer WezTerm pane's pixel origin and cell
metrics, and the file surface is painted as an opaque late overlay inside only
that rectangle. Ordinary focus changes do not reassign the owner; positional
pane selection therefore cannot make the surface jump sides. A real tmux
swap or resize changes the owning pane's snapshot and moves or resizes the
surface with it. Server-wide snapshots distinguish a pane deleted from one
merely hidden in another tmux window: changing windows hides the surface, and
returning to the owner's window restores it. Removing the owner restores
terminal mode.

The same tmux context request reads the server's global `prefix` and `prefix2`
options and records the active inner `pane_id`. In preview mode the file
workspace is keyboard-transparent: after explicit Cosmos shortcuts are
handled, every other raw key returns to the terminal input pipeline. Tmux
therefore retains its command prompt, copy mode, key tables, repeat bindings,
and direct no-prefix bindings even while the file surface is painted. The
Explorer key handler observes the same gate because selecting a file can leave
the sidebar visually focused.

`prefix+0` is the deliberate exception and enters a global Explorer keyboard
region from any inner tmux pane, even while the file surface is hidden or
owned by a different pane. Its small key set maps W/S to one-row movement,
Shift+W/S to five-row movement, A/D and arrows to tree collapse/expand,
Return to activation, and Escape to exit. Cosmos buffers the detected tmux
prefix for one key: 0 consumes the buffered prefix without sending either key
to tmux, while every other command exits Explorer focus, replays the exact
configured prefix, and continues through the normal terminal pipeline. This
keeps positional 1/2 commands reliable even when pane 1 is already the file
workspace owner.

Edit mode deliberately owns text input only while its owner is active. There,
raw input still recognizes a configured prefix before Cosmos's normal key map
encodes it and passes the prefix plus the next command key to tmux. After focus
moves away, ordinary input belongs to the newly active terminal even if the
owner remains in edit mode. Inner pane ID is presentation ownership; active
pane ID is input focus. Keeping those values separate also distinguishes panes
that share a CWD without resetting on ordinary resize.

Context requests run approximately twice per second, independently from
directory loading. This makes native focus and tmux pane changes visible
without shell hooks. OSC 7 remains useful because it improves the CWD that
WezTerm reports for native and remote-aware shells.

## File workspace

`Command+S` toggles between terminal painting and the file workspace for the
focused native or tmux pane. Once opened, the surface remains attached to that
pane while focus moves elsewhere. `Command+S` on its owner closes it;
`Command+S` on another pane deliberately transfers the single workspace to
that pane. A new pane context has no selected file; files load only through a
visible Explorer row or the `COSMOS_FILE_OPEN` shell-integration user variable.
The owning pane's exact resolved CWD is the security and display root.

Loads canonicalize both root and target, reject paths outside the root, reject
binary data, and cap editable content at 2 MiB. Markdown is parsed with
`pulldown-cmark` into native heading, paragraph, list, quote, task, rule, code,
and link-destination rows. Other UTF-8 files use a monospaced text view.

Edit mode owns keystrokes before the normal terminal input map. `Command+Return`
writes a same-directory temporary file, preserves permissions, synchronizes
it, and atomically renames it over the original. The revision captured at load
must still match immediately before save; external changes from a shell, tmux
pane, Git operation, or another editor block the write. Dirty buffers block
normal close/quit shortcuts until saved or deliberately discarded.

The file surface is window state, not a mux pane. Terminal tabs, PTYs, tmux
clients, splits, and scrollback remain allocated while it is visible.
Escape from native preview, `‹ TERMINAL`, or a terminal-tab click simply
restores terminal painting and input. In tmux preview, Escape remains tmux
input; `Command+S` or the clickable terminal control restores terminal
painting.

## Git decorations

The active display root is resolved to its containing repository on a
dedicated worker. A NUL-delimited `git status --porcelain=v1` snapshot is
parsed into absolute file paths and refreshed independently of painting.
Modified, added, deleted, renamed, untracked, and conflict states render at
the right edge of their rows. `.git` itself remains excluded, matching VS
Code's default Explorer exclusions while preserving visible dotfiles such as
`.github`, `.vscode`, and `.gitignore`. Filesystem watcher events request an
immediate status refresh; a 30-second poll is only a missed-event fallback.

## Codex status

The footer reads only structured `token_count` JSONL events under
`$CODEX_HOME/sessions` (or `~/.codex/sessions`). It never parses prompt or
response text. The active rollout's metadata is checked on the two-second UI
refresh, and at most an 8 MiB tail is read only after that file changes.
Broader rollout discovery is cached for 15 seconds, which avoids repeatedly
walking a large session history.

On macOS, active loops are counted with the native process API and require an
executable basename of exactly `codex`. Processes such as
`codex-code-mode-host` are excluded. This design does not spawn `ps`, `pgrep`,
Codex CLI calls, a daemon, or any persistent status helper.

## System capacity

The existing status request samples cumulative host CPU ticks and Mach VM
statistics no faster than once every ten seconds. CPU load is derived from
the delta between samples. RAM usage includes non-purgeable anonymous, wired,
and compressed pages while excluding reclaimable file cache; total physical
memory is cached after the first request. The sampling adds two native calls
to the existing worker cycle and does not add a timer, thread, subprocess,
daemon, filesystem read, or network request. The restrained cadence also
avoids repainting the GPU surface just to chase rapidly fluctuating counters.
Raw page-count changes that do not alter the formatted status label do not
invalidate the window.

## Protected close

Protected close is an optional compatibility integration. When a close-lock
credential exists, `Command+W` and `Command+Q` use the native
`PromptInputLine` overlay with password concealment. The overlay renders one
bullet per entered character, disables line-editor paths that could repaint
the source value, and returns the original value only to the action callback.
Escape cancels immediately without a notification.

Cosmos verifies the existing tmux-manager close-lock credential in-process
using PBKDF2-HMAC-SHA256 and constant-time comparison. It does not launch the
tmux-manager AppleScript prompt, put the password in process arguments, or log
the entered value. After successful verification, the existing tmux-manager
autosave runs before the requested close. Any user-facing failure message is
branded `Cosmos Term`.

On a clean installation with no close-lock file, `Command+W` uses WezTerm's
confirmed tab-close action and `Command+Q` uses the normal application-quit
action. This keeps a personal external integration from becoming a public
runtime dependency.

The synthetic terminal used by this overlay identifies itself as
`Cosmos Term`. Terminal state now derives its initial title from the supplied
terminal-program identity instead of a fixed `wezterm` string, and removing an
overlay refreshes the restored pane title.

Native, SSH, and tmux-backed pane terminals also initialize with the
`Cosmos Term` identity. The local-pane title enhancer recognizes both that
identity and the inherited WezTerm defaults, so a foreground process or an
application-provided OSC title can still replace the placeholder normally.

## Current-folder policy

The Explorer is permanently enabled and always follows the active pane's exact
CWD. There is no user-facing Project Follow, Locked, or hide state. Legacy
follow-mode values remain deserializable so existing state files migrate
without loss, but runtime context application forces current-folder Follow.
The header ellipsis and compatibility command actions simply reveal the active
pane again.

Directory requests are root-first and lazy. Persisted expanded descendants are
requested only after they become reachable through the currently rendered
tree. This prevents an unavailable historical descendant from delaying the
new active root.

## Persistence

Explorer state is stored atomically as JSON at:

```text
~/Library/Application Support/cosmos-term/workspace-state.json
```

`COSMOS_TERM_DATA_DIR` overrides this data root for isolated development and
tests. `COSMOS_TERM_RUNTIME_DIR` similarly overrides the socket and runtime
root. These Cosmos-only variables make it possible to exercise a second build
without reusing a live installation's state or mux endpoints.

The file contains a layout version, sidebar width, expanded directories,
hidden-file preference, and legacy roots/follow/visibility fields. The legacy
fields are retained for state compatibility but cannot hide, lock, or widen
the runtime scope beyond the active pane directory. Cached listings, Git
status, Codex/system status, and pane context are intentionally transient.

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

Cosmos also adds a narrow built-in-only font resolution entry point. Explorer
fonts are already explicitly registered, so sending each UI size and weight
through the generic CoreText locator only enumerated and parsed the complete
macOS font catalog. Direct lookup preserves the same loaded-font cache and
cap-height scaling while avoiding that transient startup allocation; unknown
fonts still use the normal upstream resolver.
