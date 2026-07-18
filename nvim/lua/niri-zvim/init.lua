local M = {}

local uv = vim.uv
local pipe
local input = ""
local revision = 0
local instance_id = string.format("nvim-%d-%d", vim.fn.getpid(), uv.hrtime())
local cached_neighbors
local topology_dirty = true
local reconnect_timer
local publish_pending = false
local terminal_focused = true
local navigation_queue = {}
local navigation_pending = false
local acknowledged_sequence
local protocol_version = 1
local enabled = false
local options = {
  reconnect_interval_ms = 250,
}

local function socket_path()
  if options.socket_path then
    return options.socket_path
  end
  if vim.env.NIRI_ZVIM_SOCKET and vim.env.NIRI_ZVIM_SOCKET ~= "" then
    return vim.env.NIRI_ZVIM_SOCKET
  end
  if vim.env.XDG_RUNTIME_DIR and vim.env.XDG_RUNTIME_DIR ~= "" then
    return vim.env.XDG_RUNTIME_DIR .. "/niri-zvim.sock"
  end
end

local function parent()
  if vim.env.ZELLIJ_SESSION_NAME and vim.env.ZELLIJ_PANE_ID then
    return {
      ZellijPane = {
        client = {
          session = vim.env.ZELLIJ_SESSION_NAME,
          client_id = 0,
        },
        pane_id = tonumber(vim.env.ZELLIJ_PANE_ID),
      },
    }
  end
  return "FocusedNiriWindow"
end

local function empty_neighbors()
  return { left = vim.NIL, down = vim.NIL, up = vim.NIL, right = vim.NIL }
end

local function rectangles()
  local result = {}
  for _, win in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
    local config = vim.api.nvim_win_get_config(win)
    if config.relative == "" then
      local number = vim.fn.win_id2win(win)
      local position = vim.fn.win_screenpos(number)
      result[#result + 1] = {
        id = win,
        x = position[2],
        y = position[1],
        width = vim.api.nvim_win_get_width(win),
        height = vim.api.nvim_win_get_height(win),
      }
    end
  end
  return result
end

local function score(current, candidate, direction)
  local cx = current.x + current.width / 2
  local cy = current.y + current.height / 2
  local ox = candidate.x + candidate.width / 2
  local oy = candidate.y + candidate.height / 2
  local current_right = current.x + current.width
  local current_bottom = current.y + current.height
  local candidate_right = candidate.x + candidate.width
  local candidate_bottom = candidate.y + candidate.height
  local vertically_overlaps = current.y < candidate_bottom and candidate.y < current_bottom
  local horizontally_overlaps = current.x < candidate_right and candidate.x < current_right
  local primary, perpendicular
  if direction == "left" and candidate_right <= current.x and vertically_overlaps then
    primary, perpendicular = current.x - candidate_right, math.abs(cy - oy)
  elseif direction == "right" and candidate.x >= current_right and vertically_overlaps then
    primary, perpendicular = candidate.x - current_right, math.abs(cy - oy)
  elseif direction == "up" and candidate_bottom <= current.y and horizontally_overlaps then
    primary, perpendicular = current.y - candidate_bottom, math.abs(cx - ox)
  elseif direction == "down" and candidate.y >= current_bottom and horizontally_overlaps then
    primary, perpendicular = candidate.y - current_bottom, math.abs(cx - ox)
  else
    return nil
  end
  return primary + perpendicular * 4
end

local function build_neighbors()
  local windows = rectangles()
  local result = {}
  for _, current in ipairs(windows) do
    local neighbors = empty_neighbors()
    for _, direction in ipairs({ "left", "down", "up", "right" }) do
      local best, best_score
      for _, candidate in ipairs(windows) do
        if candidate.id ~= current.id then
          local candidate_score = score(current, candidate, direction)
          if candidate_score and (not best_score or candidate_score < best_score) then
            best, best_score = candidate.id, candidate_score
          end
        end
      end
      neighbors[direction] = best or vim.NIL
    end
    result[tostring(current.id)] = neighbors
  end
  return result
end

local function window_neighbors()
  if topology_dirty or not cached_neighbors then
    cached_neighbors = build_neighbors()
    topology_dirty = false
  end
  return cached_neighbors
end

local function write(message)
  if pipe and pipe:is_active() then
    pipe:write(vim.json.encode({
      protocol_version = protocol_version,
      message = message,
    }) .. "\n")
  end
end

local function snapshot()
  local state = {
    id = instance_id,
    parent = parent(),
    terminal_focused = terminal_focused,
    revision = revision,
    acknowledged_sequence = acknowledged_sequence or vim.NIL,
    focused_window = vim.api.nvim_get_current_win(),
    window_neighbors = window_neighbors(),
  }
  write({ type = "nvim_snapshot", state = state })
end

local function schedule_snapshot()
  if publish_pending then
    return
  end
  publish_pending = true
  vim.schedule(function()
    publish_pending = false
    revision = revision + 1
    snapshot()
  end)
end

local function navigate(direction, neighbors_by_window)
  local focused = tostring(vim.api.nvim_get_current_win())
  local neighbors = neighbors_by_window[focused]
  local target = neighbors and neighbors[direction]
  if target and target ~= vim.NIL and vim.api.nvim_win_is_valid(target) then
    vim.api.nvim_set_current_win(target)
  end
  revision = revision + 1
end

local function queue_navigation(sequence, direction)
  navigation_queue[#navigation_queue + 1] = {
    sequence = sequence,
    direction = direction,
  }
  if navigation_pending then
    return
  end
  navigation_pending = true
  vim.schedule(function()
    navigation_pending = false
    local queued = navigation_queue
    navigation_queue = {}
    local neighbors = window_neighbors()
    for _, queued_navigation in ipairs(queued) do
      navigate(queued_navigation.direction, neighbors)
      acknowledged_sequence = queued_navigation.sequence
    end
    snapshot()
  end)
end

local function consume(data)
  input = input .. data
  while true do
    local newline = input:find("\n", 1, true)
    if not newline then
      break
    end
    local line = input:sub(1, newline - 1)
    input = input:sub(newline + 1)
    local ok, message = pcall(vim.json.decode, line)
    if ok
        and message.protocol_version == protocol_version
        and type(message.message) == "table"
        and message.message.type == "navigate" then
      queue_navigation(message.message.sequence, message.message.direction)
    end
  end
end

local function reconnect()
  if not enabled or reconnect_timer then
    return
  end
  local timer = uv.new_timer()
  reconnect_timer = timer
  timer:start(options.reconnect_interval_ms, 0, vim.schedule_wrap(function()
    timer:close()
    if reconnect_timer == timer then
      reconnect_timer = nil
    end
    if enabled then
      M.connect()
    end
  end))
end

function M.connect()
  if not enabled then
    return
  end
  local path = socket_path()
  if not path then
    vim.notify_once("niri-zvim requires XDG_RUNTIME_DIR or NIRI_ZVIM_SOCKET", vim.log.levels.ERROR)
    return
  end
  if pipe and not pipe:is_closing() then
    pipe:close()
  end
  local connection = uv.new_pipe(false)
  pipe = connection
  connection:connect(path, function(error)
    if pipe ~= connection or not enabled then
      if not connection:is_closing() then
        connection:close()
      end
      return
    end
    if error then
      connection:close()
      reconnect()
      return
    end
    connection:write(string.char(0x7f, protocol_version))
    connection:read_start(function(read_error, data)
      if read_error or not data then
        if not connection:is_closing() then
          connection:close()
        end
        if pipe == connection then
          pipe = nil
        end
        reconnect()
        return
      end
      consume(data)
    end)
    vim.schedule(snapshot)
  end)
end

local function validate_options(user_options)
  if user_options == nil then
    user_options = {}
  end
  if type(user_options) ~= "table" then
    error("niri-zvim setup options must be a table")
  end
  for key in pairs(user_options) do
    if key ~= "enabled" and key ~= "socket_path" and key ~= "reconnect_interval_ms" then
      error("unknown niri-zvim setup option: " .. key)
    end
  end
  if user_options.enabled ~= nil and type(user_options.enabled) ~= "boolean" then
    error("niri-zvim enabled must be a boolean")
  end
  if user_options.socket_path ~= nil
      and (type(user_options.socket_path) ~= "string"
        or user_options.socket_path == ""
        or user_options.socket_path:sub(1, 1) ~= "/") then
    error("niri-zvim socket_path must be a non-empty absolute path")
  end
  if user_options.reconnect_interval_ms ~= nil
      and (type(user_options.reconnect_interval_ms) ~= "number"
        or user_options.reconnect_interval_ms < 1
        or user_options.reconnect_interval_ms % 1 ~= 0) then
    error("niri-zvim reconnect_interval_ms must be a positive integer")
  end
  return {
    enabled = user_options.enabled ~= false,
    socket_path = user_options.socket_path,
    reconnect_interval_ms = user_options.reconnect_interval_ms or 250,
  }
end

function M.disable()
  if enabled then
    write({ type = "nvim_closed", id = instance_id })
  end
  enabled = false
  pcall(vim.api.nvim_del_augroup_by_name, "niri_zvim")
  if reconnect_timer then
    reconnect_timer:stop()
    reconnect_timer:close()
    reconnect_timer = nil
  end
  if pipe and not pipe:is_closing() then
    pipe:read_stop()
    pipe:close()
  end
  pipe = nil
  input = ""
  navigation_queue = {}
  navigation_pending = false
end

function M.setup(user_options)
  local configured = validate_options(user_options)
  M.disable()
  options = configured
  if not configured.enabled then
    return M
  end
  enabled = true
  local group = vim.api.nvim_create_augroup("niri_zvim", { clear = true })
  vim.api.nvim_create_autocmd({
    "WinNew",
    "WinClosed",
    "WinResized",
    "TabEnter",
    "TabNew",
    "TabClosed",
    "VimResized",
  }, {
    group = group,
    callback = function()
      topology_dirty = true
      schedule_snapshot()
    end,
  })
  vim.api.nvim_create_autocmd("WinEnter", {
    group = group,
    callback = schedule_snapshot,
  })
  vim.api.nvim_create_autocmd("VimLeavePre", {
    group = group,
    callback = function()
      write({ type = "nvim_closed", id = instance_id })
    end,
  })
  vim.api.nvim_create_autocmd("FocusGained", {
    group = group,
    callback = function()
      terminal_focused = true
      schedule_snapshot()
    end,
  })
  vim.api.nvim_create_autocmd("FocusLost", {
    group = group,
    callback = function()
      terminal_focused = false
      schedule_snapshot()
    end,
  })
  M.connect()
  return M
end

function M.enable()
  if enabled then
    return M
  end
  return M.setup({
    enabled = true,
    socket_path = options.socket_path,
    reconnect_interval_ms = options.reconnect_interval_ms,
  })
end

function M.is_enabled()
  return enabled
end

return M
