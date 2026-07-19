vim.opt.loadplugins = false
vim.opt.shadafile = "NONE"
vim.opt.swapfile = false
vim.opt.splitright = true

local source = debug.getinfo(1, "S").source:sub(2)
local project_dir = vim.fs.dirname(vim.fs.dirname(vim.fs.dirname(source)))
vim.opt.runtimepath:prepend(project_dir .. "/nvim")

local benchmark_plugin = vim.env.NIRI_ZVIM_BENCH_VIM_PLUGIN
if benchmark_plugin ~= nil and benchmark_plugin ~= "" then
  vim.cmd("source " .. vim.fn.fnameescape(benchmark_plugin))
end

require("niri-zvim").setup()
