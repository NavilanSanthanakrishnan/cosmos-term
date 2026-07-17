# Verified Learnings

- The compatibility baseline is WezTerm commit
  `5046fc225992db6ba2ef8812743fadfdfe4b184a`, selected because it exactly
  matches the WezTerm installation present when Cosmos Term was forked.
- Standalone identity requires changing more than the bundle name: config
  discovery, data/cache/runtime paths, socket and pane environment variables,
  companion executable lookup, window class, log identity, and CLI defaults
  must all be distinct.
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
- Normal tab close should remain independent from whole-application protection:
  bind `Command+W` directly to `CloseCurrentTab`, and reserve close-lock plus
  autosave for `Command+Q`.
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
