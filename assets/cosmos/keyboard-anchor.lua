local wezterm = require 'wezterm'
local act = wezterm.action

local M = {}
local trace_path = os.getenv('HOME') .. '/Library/Logs/KeyboardAnchor/cosmos-term-actions.jsonl'

local modifier_order = { 'SUPER', 'CTRL', 'ALT', 'SHIFT', 'LEADER' }
local modifier_aliases = {
  CMD = 'SUPER',
  WIN = 'SUPER',
  OPT = 'ALT',
  META = 'ALT',
}
local shifted_printable = {
  ['`'] = '~',
  ['1'] = '!',
  ['2'] = '@',
  ['3'] = '#',
  ['4'] = '$',
  ['5'] = '%',
  ['6'] = '^',
  ['7'] = '&',
  ['8'] = '*',
  ['9'] = '(',
  ['0'] = ')',
  ['-'] = '_',
  ['='] = '+',
  ['['] = '{',
  [']'] = '}',
  ['\\'] = '|',
  [';'] = ':',
  ["'"] = '"',
  [','] = '<',
  ['.'] = '>',
  ['/'] = '?',
}

local function normalize_mods(mods)
  local present = {}
  for modifier in string.gmatch(mods or '', '[^|]+') do
    modifier = modifier_aliases[modifier] or modifier
    if modifier ~= '' and modifier ~= 'NONE' then
      present[modifier] = true
    end
  end

  local result = {}
  for _, modifier in ipairs(modifier_order) do
    if present[modifier] then
      table.insert(result, modifier)
      present[modifier] = nil
    end
  end
  for modifier in pairs(present) do
    table.insert(result, modifier)
  end
  return #result == 0 and 'NONE' or table.concat(result, '|')
end

local function binding_id(key, mods)
  key = string.gsub(key, '^mapped:', '')
  key = string.gsub(key, '^phys:', '')
  return normalize_mods(mods) .. '\t' .. key
end

local function binding_ids(key, mods)
  local keys = { key }
  if string.find(normalize_mods(mods), 'SHIFT', 1, true)
      and string.match(key, '^%a$') then
    table.insert(keys, string.upper(key))
    table.insert(keys, string.lower(key))
  end

  local result = {}
  local seen = {}
  for _, candidate in ipairs(keys) do
    local id = binding_id(candidate, mods)
    if not seen[id] then
      table.insert(result, id)
      seen[id] = true
    end
  end
  return result
end

local function add_bindings(assignments, bindings, source)
  for _, binding in ipairs(bindings or {}) do
    assignments[binding_id(binding.key, binding.mods)] = {
      action = binding.action,
      source = source,
    }
  end
end

local function find_action(assignments, key, mods)
  for _, id in ipairs(binding_ids(key, mods)) do
    if assignments[id] then
      return assignments[id]
    end
  end
end

local function trace_action(fields)
  local file = io.open(trace_path, 'a')
  if not file then
    return
  end
  file:write(wezterm.json_encode(fields), '\n')
  file:close()
end

local function fallback_key_and_mods(key, mods)
  if normalize_mods(mods) ~= 'SHIFT' then
    return key, mods, 'send_key_fallback'
  end
  if shifted_printable[key] then
    return shifted_printable[key], 'NONE', 'send_key_shifted_fallback'
  end
  if string.match(key, '^%a$') then
    return string.upper(key), 'NONE', 'send_key_shifted_fallback'
  end
  return key, mods, 'send_key_fallback'
end

function M.apply(config)
  local root_assignments = {}
  local table_assignments = {}
  local defaults_loaded = false
  local custom_keys = config.keys or {}
  local custom_key_tables = config.key_tables or {}

  local function load_assignments()
    if not defaults_loaded and wezterm.gui then
      add_bindings(root_assignments, wezterm.gui.default_keys(), 'default')
      for name, bindings in pairs(wezterm.gui.default_key_tables()) do
        table_assignments[name] = table_assignments[name] or {}
        add_bindings(table_assignments[name], bindings, 'default_key_table:' .. name)
      end
      defaults_loaded = true
      add_bindings(root_assignments, custom_keys, 'custom')
      for name, bindings in pairs(custom_key_tables) do
        table_assignments[name] = table_assignments[name] or {}
        add_bindings(table_assignments[name], bindings, 'custom_key_table:' .. name)
      end
    end
  end
  add_bindings(root_assignments, custom_keys, 'custom')

  wezterm.on('user-var-changed', function(window, pane, name, value)
    if name ~= 'keyboard_anchor_action' then
      return
    end

    local fields = {}
    for field in string.gmatch(value .. '\t', '(.-)\t') do
      table.insert(fields, field)
    end
    if #fields < 2 then
      return
    end
    local sequence = 'manual'
    local mods
    local key
    local tracing = false
    if #fields >= 3 then
      sequence = fields[1]
      mods = normalize_mods(fields[2])
      key = fields[3]
      tracing = fields[4] == '1'
    else
      -- Accept the original two-field format for manual diagnostics.
      mods = normalize_mods(fields[1])
      key = fields[2]
    end

    local target_pane = window:active_pane() or pane
    load_assignments()
    local resolved
    local active_table = window:active_key_table()
    if active_table and table_assignments[active_table] then
      resolved = find_action(table_assignments[active_table], key, mods)
    end
    resolved = resolved or find_action(root_assignments, key, mods)
    local action
    local source
    if resolved then
      action = resolved.action
      source = resolved.source
    else
      local fallback_key
      local fallback_mods
      fallback_key, fallback_mods, source = fallback_key_and_mods(key, mods)
      action = act.SendKey {
        key = fallback_key,
        mods = fallback_mods == 'NONE' and '' or fallback_mods,
      }
    end
    local ok, action_error = pcall(function()
      window:perform_action(action, target_pane)
    end)
    if tracing then
      trace_action {
        timestamp = wezterm.strftime('%Y-%m-%dT%H:%M:%S%z'),
        sequence = sequence,
        key = key,
        modifiers = mods,
        active_key_table = active_table or '',
        resolution = source,
        outcome = ok and 'ok' or 'error',
        detail = ok and '' or tostring(action_error),
      }
    end
  end)

  return config
end

return M
