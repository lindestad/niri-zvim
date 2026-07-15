if vim.g.loaded_niri_zvim then
  return
end
vim.g.loaded_niri_zvim = true

require("niri-zvim").setup()
