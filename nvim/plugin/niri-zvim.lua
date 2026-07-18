if vim.g.loaded_niri_zvim then
  return
end
vim.g.loaded_niri_zvim = true

vim.api.nvim_create_user_command("NiriZvimEnable", function()
  require("niri-zvim").enable()
end, {})

vim.api.nvim_create_user_command("NiriZvimDisable", function()
  require("niri-zvim").disable()
end, {})
