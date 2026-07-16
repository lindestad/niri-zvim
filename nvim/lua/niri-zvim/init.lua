local M = {}

local uv = vim.uv
local pipe
local input = ""
local revision = 0
local instance_id = string.format("nvim-%d-%d", vim.fn.getpid(), uv.hrtime())
local current_state
local reconnect_timer
local publish_pending = false
local terminal_focused = false

local function socket_path()
  return vim.env.NIRI_ZVIM_SOCKET
    or ((vim.env.XDG_RUNTIME_DIR or "/tmp") .. "/niri-zvim.sock")
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
  local primary, perpendicular
  if direction == "left" and ox < cx then
    primary, perpendicular = cx - ox, math.abs(cy - oy)
  elseif direction == "right" and ox > cx then
    primary, perpendicular = ox - cx, math.abs(cy - oy)
  elseif direction == "up" and oy < cy then
    primary, perpendicular = cy - oy, math.abs(cx - ox)
  elseif direction == "down" and oy > cy then
    primary, perpendicular = oy - cy, math.abs(cx - ox)
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

local function write(message)
  if pipe and pipe:is_active() then
    pipe:write(vim.json.encode(message) .. "\n")
  end
end

local function snapshot()
  current_state = {
    id = instance_id,
    parent = parent(),
    terminal_focused = terminal_focused,
    revision = revision,
    focused_window = vim.api.nvim_get_current_win(),
    window_neighbors = build_neighbors(),
  }
  write({ type = "nvim_snapshot", state = current_state })
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

local function navigate(direction)
  if not current_state then
    snapshot()
  end
  local focused = tostring(current_state.focused_window)
  local neighbors = current_state.window_neighbors[focused]
  local target = neighbors and neighbors[direction]
  if target and target ~= vim.NIL and vim.api.nvim_win_is_valid(target) then
    vim.api.nvim_set_current_win(target)
  end
  revision = revision + 1
  snapshot()
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
    if ok and message.type == "navigate" then
      vim.schedule(function()
        navigate(message.direction)
      end)
    end
  end
end

local function reconnect()
  if reconnect_timer then
    return
  end
  reconnect_timer = uv.new_timer()
  reconnect_timer:start(250, 0, vim.schedule_wrap(function()
    reconnect_timer:close()
    reconnect_timer = nil
    M.connect()
  end))
end

function M.connect()
  if pipe and not pipe:is_closing() then
    pipe:close()
  end
  pipe = uv.new_pipe(false)
  pipe:connect(socket_path(), function(error)
    if error then
      pipe:close()
      reconnect()
      return
    end
    pipe:write(string.char(0x7f))
    pipe:read_start(function(read_error, data)
      if read_error or not data then
        if not pipe:is_closing() then
          pipe:close()
        end
        reconnect()
        return
      end
      consume(data)
    end)
    vim.schedule(snapshot)
  end)
end

function M.setup()
  local group = vim.api.nvim_create_augroup("niri_zvim", { clear = true })
  vim.api.nvim_create_autocmd({
    "WinEnter",
    "WinNew",
    "WinClosed",
    "WinResized",
    "TabEnter",
    "TabNew",
    "TabClosed",
    "VimResized",
  }, {
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
end

return M
