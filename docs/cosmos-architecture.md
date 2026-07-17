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
- compatibility loading and migration of legacy workspace-root/follow state
- exact active-pane-directory scoping
- project-root discovery for Git decorations
- lazy direct-directory listings and stable sorting
- row generation for expanded directories
- pane-context resolution for native panes and tmux
- non-blocking Git status snapshots and porcelain parsing
- read-only Codex usage snapshots and native active-process counting
- filesystem change notification

Four independent worker threads serve directory reads, pane-context requests,
Git status requests, and filesystem watches. Responses return through a single
non-blocking channel consumed by the window. The render thread never
recursively scans a workspace, invokes Git, or reads Codex session data. Codex
status work shares the existing pane-context worker rather than creating a
fifth thread or helper process.

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

- constructing and polling the explorer from `TermWindow`
- offsetting render/layout coordinates
- routing explorer mouse and keyboard events, including explicit focus return
  when a terminal pane is clicked
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
tmux display-message -p -t <client-tty> "#{pane_current_path}"
```

The process inherits the GUI's `TMUX` environment and therefore queries the
same server as the attached client. Cosmos invokes the foreground tmux
process's full executable path when available, so a Finder-launched GUI does
not depend on Homebrew being present in its reduced `PATH`. If the query
fails, the explorer keeps the terminal usable, falls back to reported or
last-known context, and displays the error in the sidebar.

Context requests run approximately twice per second, independently from
directory loading. This makes native focus and tmux pane changes visible
without shell hooks. OSC 7 remains useful because it improves the CWD that
WezTerm reports for native and remote-aware shells.

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

## Protected close

`Command+W` and `Command+Q` use the native `PromptInputLine` overlay with
password concealment. The overlay renders one bullet per entered character,
disables line-editor paths that could repaint the source value, and returns
the original value only to the action callback. Escape cancels immediately
without a notification.

Cosmos verifies the existing tmux-manager close-lock credential in-process
using PBKDF2-HMAC-SHA256 and constant-time comparison. It does not launch the
tmux-manager AppleScript prompt, put the password in process arguments, or log
the entered value. After successful verification, the existing tmux-manager
autosave runs before the requested close. Any user-facing failure message is
branded `Cosmos Term`.

The synthetic terminal used by this overlay identifies itself as
`Cosmos Term`. Terminal state now derives its initial title from the supplied
terminal-program identity instead of a fixed `wezterm` string, and removing an
overlay refreshes the restored pane title.

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

The file contains a layout version, sidebar width, expanded directories,
hidden-file preference, and legacy roots/follow/visibility fields. The legacy
fields are retained for state compatibility but cannot hide, lock, or widen
the runtime scope beyond the active pane directory. Cached listings, Git
status, Codex status, and pane context are intentionally transient.

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
