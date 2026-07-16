vim.opt.loadplugins = false
vim.opt.shadafile = "NONE"
vim.opt.swapfile = false
vim.opt.splitright = true

local source = debug.getinfo(1, "S").source:sub(2)
local project_dir = vim.fs.dirname(vim.fs.dirname(vim.fs.dirname(source)))
vim.opt.runtimepath:prepend(project_dir .. "/nvim")

require("niri-zvim").setup()
