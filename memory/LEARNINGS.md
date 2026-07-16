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
