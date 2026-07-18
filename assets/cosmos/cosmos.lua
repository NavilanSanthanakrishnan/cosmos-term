local wezterm = require 'wezterm'

local config = wezterm.config_builder()
local act = wezterm.action
local home = os.getenv 'HOME' or ''
local tmux_manager_bin = os.getenv('TMUX_MANAGER_BIN')
  or (home .. '/.local/bin/tmux-manager')
local autosave_command = {
  tmux_manager_bin,
  'autosave',
  'workspace',
}
local tmux_manager_state_dir = os.getenv('TMUX_MANAGER_STATE_DIR')
  or (home .. '/.local/state/tmux-manager')
local close_lock_path = os.getenv('COSMOS_TERM_CLOSE_LOCK_FILE')
  or (tmux_manager_state_dir:gsub('/+$', '') .. '/close-lock.json')

local function file_exists(path)
  local file = io.open(path, 'rb')
  if file == nil then
    return false
  end
  file:close()
  return true
end

-- Protected close is an optional local integration. Existing users with a
-- close-lock credential retain the password + autosave flow; a clean install
-- uses WezTerm's standard confirmation and has no external dependency.
local protected_close_enabled = file_exists(close_lock_path)

-- Cosmos is a terminal workbench rather than a compact terminal window. These
-- defaults leave enough room for the persistent explorer and make both the
-- terminal and native sidebar labels comfortable to scan.
-- WebGPU's sRGB surface preserves CSS theme colors exactly on both standard
-- and wide-gamut macOS displays; the legacy OpenGL path applies the display
-- profile a second time and visibly lifts VS Code's #252526 sidebar color.
config.front_end = 'WebGpu'
config.font_size = 14.0
config.line_height = 1.08
config.initial_cols = 100
config.initial_rows = 32

local function protected_save_then(action, target)
  return act.PromptInputLine {
    description = wezterm.format {
      { Attribute = { Intensity = 'Bold' } },
      { Foreground = { Color = '#cccccc' } },
      { Text = 'COSMOS TERM CLOSE LOCK\n' },
      { Attribute = { Intensity = 'Normal' } },
      { Foreground = { Color = '#9d9d9d' } },
      { Text = 'Enter your close password to permanently close ' .. target .. '.\n' },
      { Text = 'Press Escape to cancel. Your input is hidden.' },
    },
    password = true,
    action = wezterm.action_callback(function(window, pane, passphrase)
      if passphrase == nil then
        return
      end

      local verified, unlocked = pcall(
        wezterm.gui.verify_close_lock,
        close_lock_path,
        passphrase
      )
      passphrase = nil
      if not verified then
        wezterm.log_error('Cosmos Term close-lock credential is unavailable or invalid')
        window:toast_notification(
          'Cosmos Term',
          'Close blocked: the close password is not configured correctly.',
          nil,
          5000
        )
        return
      end
      if not unlocked then
        window:toast_notification(
          'Cosmos Term',
          'Incorrect password. Nothing was closed.',
          nil,
          3500
        )
        return
      end

      local success, _, stderr = wezterm.run_child_process(autosave_command)
      if not success then
        wezterm.log_error('tmux-manager autosave failed: ' .. stderr)
        window:toast_notification(
          'Cosmos Term',
          'The pre-close snapshot failed; the periodic backup is still available.',
          nil,
          5000
        )
      end
      window:perform_action(action, pane)
    end),
  }
end

-- Title-bar/window-manager closes still receive the built-in confirmation.
-- Command+W permanently closes the current tab and all of its panes only
-- after the custom close-lock succeeds and the workspace has been saved.
-- Command+Q applies the same protection to the whole application.
config.window_close_confirmation = 'AlwaysPrompt'

-- Send Shift+Backspace as a distinct xterm modified-key sequence so tmux can
-- use it as the prefix without taking over ordinary Backspace.
local close_tab_action = act.CloseCurrentTab { confirm = true }
local quit_action = act.QuitApplication
if protected_close_enabled then
  close_tab_action = protected_save_then(
    act.CloseCurrentTab { confirm = false },
    'this tab and all of its processes'
  )
  quit_action = protected_save_then(act.QuitApplication, 'Cosmos Term')
end

config.keys = {
  {
    key = 'Backspace',
    mods = 'SHIFT',
    action = wezterm.action.SendString '\x1b[27;2;127~',
  },
  {
    key = 'w',
    mods = 'SUPER',
    action = close_tab_action,
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
    action = quit_action,
  },
}

return dofile(wezterm.executable_dir .. '/../Resources/keyboard-anchor.lua').apply(config)
