local wezterm = require 'wezterm'

local config = wezterm.config_builder()
local act = wezterm.action
local autosave_command = {
  '/Users/navilan/.local/bin/tmux-manager',
  'autosave',
  'workspace',
}
local close_lock_command = {
  '/Users/navilan/.local/bin/tmux-manager',
  'close-lock',
  'verify',
  '--gui',
}

-- Cosmos is a terminal workbench rather than a compact terminal window. These
-- defaults leave enough room for the persistent explorer and make both the
-- terminal and native sidebar labels comfortable to scan.
config.font_size = 14.0
config.line_height = 1.08
config.initial_cols = 100
config.initial_rows = 32

local function protected_save_then(action)
  return wezterm.action_callback(function(window, pane)
    local unlocked, _, unlock_error = wezterm.run_child_process(close_lock_command)
    if not unlocked then
      wezterm.log_warn('tmux-manager blocked Cosmos Term close: ' .. unlock_error)
      window:toast_notification(
        'tmux Manager',
        'Close blocked: the passphrase was canceled, incorrect, or is not configured.',
        nil,
        4000
      )
      return
    end
    local success, _, stderr = wezterm.run_child_process(autosave_command)
    if not success then
      wezterm.log_error('tmux-manager autosave failed: ' .. stderr)
      window:toast_notification(
        'tmux Manager',
        'The pre-close snapshot failed; the periodic backup is still available.',
        nil,
        5000
      )
    end
    window:perform_action(action, pane)
  end)
end

-- Title-bar/window-manager closes still receive the built-in confirmation.
-- Command+W is the standard immediate tab close. Command+Q retains the
-- protected autosave flow for closing the whole application.
config.window_close_confirmation = 'AlwaysPrompt'

-- Send Shift+Backspace as a distinct xterm modified-key sequence so tmux can
-- use it as the prefix without taking over ordinary Backspace.
config.keys = {
  {
    key = 'Backspace',
    mods = 'SHIFT',
    action = wezterm.action.SendString '\x1b[27;2;127~',
  },
  {
    key = 'w',
    mods = 'SUPER',
    action = act.CloseCurrentTab { confirm = false },
  },
  {
    -- The explorer is a permanent workbench region. Consume the legacy
    -- toggle chord so it cannot hide the sidebar or leak an "E" to the shell.
    key = 'e',
    mods = 'SUPER|SHIFT',
    action = act.Nop,
  },
  {
    key = 'q',
    mods = 'SUPER',
    action = protected_save_then(act.QuitApplication),
  },
}

return dofile(wezterm.executable_dir .. '/../Resources/keyboard-anchor.lua').apply(config)
