# Verified Learnings

- The compatibility baseline is WezTerm commit
  `5046fc225992db6ba2ef8812743fadfdfe4b184a`, selected because it exactly
  matches the WezTerm installation present when Cosmos Term was forked.
- Standalone identity requires changing more than the bundle name: config
  discovery, data/cache/runtime paths, socket and pane environment variables,
  companion executable lookup, window class, log identity, and CLI defaults
  must all be distinct. Terminal model constructors are part of that surface:
  their program identity becomes the initial pane, tab, and native window
  title before a shell emits its own title.
- Provide Cosmos-specific data and runtime directory overrides for isolated
  build tests. They allow a second GUI/CLI process to run without reading or
  writing the active installation's state or sockets; never repurpose
  `WEZTERM_*` variables for this.
- Clear inherited WezTerm protocol/config variables at Cosmos bootstrap. Keep a
  GUI-level `TMUX` value available for pane lookup when Cosmos was launched
  from that server, but strip `TMUX`, `TMUX_PANE`, and `WEZTERM_*` from newly
  spawned terminal commands so shells never inherit a stale parent attachment.
- tmux pane CWD can be resolved reliably by detecting `tmux` as the terminal
  pane's foreground process and querying `tmux display-message -p -t <tty>
  "#{pane_current_path}"`. The resolver must inherit the GUI's `TMUX`
  environment so it addresses the same server.
- Use the foreground tmux process's executable path for resolver queries.
  Finder-launched macOS applications may not have Homebrew's directory in
  `PATH`.
- Keep directory reads, pane-context resolution, and filesystem watching on
  separate workers. A large or inaccessible directory must not delay terminal
  rendering or active-pane tracking.
- Explorer rendering shares WezTerm's quad layers. Surface, header, and row
  backgrounds must remain below text; placing those backgrounds on a later
  layer hides otherwise correctly shaped glyphs.
- `render_screen_line` retains terminal-cell placement even when supplied a
  proportional font. Native sidebar labels need box-model shaping and direct
  glyph quad emission. Snap screen-space glyph origins to whole pixels before
  applying the centered render offset; this materially improves small UI
  labels. This baseline's CoreText locator does not enumerate Apple's private
  system-font alias, but its FreeType parser can register the OS-owned
  `/System/Library/Fonts/SFNS.ttf` face as `System Font` without redistributing
  it.
- Do not send an explicitly registered UI font through the generic platform
  locator. On macOS that path can enumerate and parse the complete CoreText
  catalog for every size/weight variant, producing a nearly 1 GiB transient
  startup peak. Resolve known built-in faces directly, retain the normal font
  cache and cap-height scaling, and fall back to platform lookup only when the
  face is not registered.
- For exact VS Code file icons, embed Code OSS's Seti font and retain its MIT
  license; visually similar Nerd Font glyphs use different outlines and
  codepoints. Keep Git status invocation on its own worker and parse
  NUL-delimited porcelain output so spaces and renames remain safe.
- Keep the active Explorer display root transient and separate from persisted
  legacy multi-root state. Runtime context application must force the active
  pane's exact CWD as the sole root so historical roots, parents, and siblings
  cannot leak into the tree.
- Treat Explorer geometry as logical CSS pixels and scale the complete layout
  at the window DPI. Scaling only the font or only row constants causes overlap
  or a tiny sidebar when moving between standard and Retina displays.
- Native CoreGraphics captures are required for renderer validation; app
  previews can hide DPI and color-profile errors. On this macOS baseline,
  WebGPU's sRGB surface emits the VS Code `#252526` sidebar value exactly,
  while the legacy OpenGL path visibly lifts it on wide-gamut displays.
- On mixed-DPI macOS setups, provisional pixel geometry can be interpreted as
  AppKit points before Cocoa assigns the final screen, and `isZoomed` can be
  transiently true during that placement. Use the active screen DPI for the
  initial pixel-to-point conversion, then reconcile the requested terminal
  cells after native placement/unzoom settles and invalidate the resized
  WebGPU surface.
- Queue the active display root before any expanded descendants. Derive later
  requests from directories reachable through the currently rendered tree so
  an unavailable stale expansion cannot block the current CWD.
- Never queue directory reads from the Explorer paint path. Gate initial
  requests on cache presence, explicitly force only watcher/periodic/user
  refreshes, and queue newly reachable persisted descendants when a parent
  listing arrives. Otherwise every terminal repaint can trigger filesystem
  work and make input feel laggy. Keep worker-response polling faster than pane
  context polling, and use `CachePolicy::AllowStale` so process discovery never
  stalls the UI thread.
- Cache generated Explorer rows and invalidate them only for visual state
  changes. Use a fast service-response interval only while work is pending,
  back off while idle, and let watcher events drive directory and Git updates
  with a slow periodic fallback. Reapplying unchanged pane context or Git
  state at timer frequency is avoidable idle CPU.
- A custom `Command+W` assignment bypasses WezTerm's window-close
  confirmation. Wrap `CloseCurrentTab` in the close-lock verifier and
  pre-close autosave callback when destructive tab closure must require the
  same custom passphrase as `Command+Q`.
- Personal integrations must be capability-detected rather than public
  dependencies. Enable protected close only when its credential exists; use
  normal confirmed close and quit actions on a clean installation.
- Do not delegate a product-owned password prompt to an AppleScript helper:
  macOS exposes the helper's name in the dialog and notifications. A concealed
  in-app prompt plus in-process PBKDF2 verification keeps the close password
  out of process arguments, logs, line history, and external app identity.
  Canceling an overlay must also refresh the window title from the restored
  pane.
- A live Codex footer does not need a daemon or CLI subprocess. Read only
  structured `token_count` lines from the changed rollout tail, cache broader
  session discovery, and use native process enumeration with an exact `codex`
  executable-basename match. This keeps multi-gigabyte session histories and
  similarly named helper processes from making the UI noisy or inaccurate.
  Keep rollout candidates bounded and reuse the macOS process-path buffer
  across PIDs so each two-second count does not allocate once per process.
- System capacity belongs in the same cached footer request. On macOS,
  cumulative host CPU ticks and Mach VM statistics provide CPU and meaningful
  occupied RAM with two native calls per refresh; cache the host port, page
  size, and total memory, derive CPU from tick deltas, and keep the sample
  cadence slower than rapidly changing text would repaint the full GPU
  surface. Do not add a helper, timer, thread, filesystem poll, or broad
  process scan for these aggregate values.
- Canvas-drawn explorer focus must be released explicitly when a terminal pane
  is clicked. Native window focus alone does not identify which in-window
  region owns keyboard events; without the handoff, `.` and Return can trigger
  explorer actions while the user is typing a shell command.
- A persistent sidebar should consume its retired toggle chord with `Nop`.
  Merely removing the default assignment can forward the modified character to
  the terminal on this baseline.
- Current Rust requires null-safe FreeType bitmap and outline slice handling
  for this older upstream baseline. The fixes match later upstream WezTerm
  behavior.
- The old `glium` version in this baseline is miscompiled in its OpenGL texture
  path by the current release optimizer. `[profile.release.package.glium]
  opt-level = 0` fixes rendering while leaving the rest of the release at
  optimization level 3.
- macOS 26 no longer exposes the deprecated `NSWindowFullScreenButton`
  constant used by this baseline; omitting that collection behavior matches
  later upstream WezTerm.
- Live tmux testing must use a dedicated socket and explicit `tmux -S` commands.
  Always compare default tmux clients before and after the test.
- Before publishing a long-lived fork, replace inherited release, scheduled,
  issue-automation, and Pages workflows with a minimal fork-owned CI surface.
  Preserve upstream licensing and history, but do not let obsolete upstream
  automation publish or mutate the fork under the wrong product assumptions.
- A terminal-integrated file workspace should be presentation state, not a
  replacement mux pane. Continue painting all terminal panes, then cover only
  the active pane rectangle with a late opaque file-workspace surface and
  route input there. Retain the original pane and its processes so returning
  is immediate and tmux state is never reconstructed.
- Pane-aware tmux presentation requires geometry as well as CWD. Query
  `pane_left`, `pane_top`, `pane_width`, and `pane_height` with
  `pane_current_path`, translate those cells through the outer native pane's
  origin and cell metrics, and keep file-workspace backgrounds and glyphs on
  the same late quad layer so covered terminal text and cursors cannot bleed
  through.
- Pane focus and presentation ownership are different state. Key the file
  surface to the inner tmux `pane_id` where it was explicitly opened, query
  snapshots for every pane on that server, and refresh the owner's geometry
  without reassigning it when active focus changes. This keeps positional
  selection stable while still following real swaps, resizes, and deletion.
  An inactive owner must not capture terminal input even when its surface is
  in edit mode. Use server-wide pane snapshots plus a window ID so an inactive
  tmux window can hide and later restore its surface instead of looking like a
  deleted owner.
- A tmux file preview should be a visual overlay, not a competing keyboard
  mode. After handling explicit product shortcuts, return all other preview
  keys to the normal terminal pipeline so command prompts, copy mode, key
  tables, repeat bindings, and `bind-key -n` bindings remain functional.
  Apply the same bypass to sidebar key handling because clicking a file can
  leave the Explorer focused after the preview opens.
  Edit mode still needs raw-prefix recognition because it intentionally owns
  text: a configured prefix such as `S-BSpace` may otherwise be converted by
  the application key map before workspace input runs. Tracking `pane_id`
  separately prevents equal-CWD panes from being treated as one context
  without resetting on ordinary resize.
- Keep recursive file search, canonicalized load, and atomic save together on
  an on-demand worker. Canonicalize both root and target, reject paths outside
  the active root, cap file size and search work, avoid symlink traversal, and
  compare a load-time revision immediately before rename so an editor cannot
  silently overwrite external changes.
- macOS Command chords can arrive as uppercase `KeyCode::Char` values even
  without an explicit Shift modifier. Native workspace shortcuts should match
  both cases before allowing unrecognized Command chords to continue through
  the normal application key map.
- Cache width-wrapped Markdown display lines independently from parsed
  document lines. Rebuild the cache only when content or viewport columns
  change; wrapping prose every paint is avoidable renderer work, while code
  should remain unwrapped and monospace.
- Treat a file workspace toggle and a file picker as separate product
  concepts. If the Explorer is the sole picker, entering file mode should
  create a blank pane-context-bound surface and never infer or search for a
  file. Reconcile both native pane identity and resolved tmux CWD; reset only
  clean state, and retain dirty buffers until the user saves or discards them.
- Avoid relying on multi-modifier character chords in this macOS input
  baseline for product-critical actions: synthetic `Command+Shift+S` and
  `Command+Shift+D` can lose the Command bit before raw-key routing. Prefer a
  tested single-modifier chord such as `Command+Return` for explicit save.
