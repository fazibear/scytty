-- Copy to ~/.config/scytty/config.lua (or $XDG_CONFIG_HOME/scytty/config.lua).
return {
  font_size = 16,
  default_cursor_style = "block", -- block, bar, underline or block_hollow.
  scrollback_mb = 16, -- Per-tab MiB, 0–64; 0 disables scrollback.
  font_family = "monospace", -- Or an installed family name.
  icon_scale = 1.0, -- Nerd Font icons followed by a space (1–3).
  background_opacity = 0.7,
  window_width = 960,
  window_height = 640,
  -- Strings or {text = "←", color = "#cba6f7"}; color is optional.
  tab_left_indicator = "<",
  tab_right_indicator = ">",
  new_tab_button = "+",

  -- Overrides default shortcuts; empty actions pass the key to the terminal.
  -- key.SUPER = Cmd on macOS, Windows/Super elsewhere. Apply with Reload config.
  key_bindings = {
    -- { keys = {key.SUPER, key.SHIFT, key.N}, actions = {"new_window"} },
    -- { keys = {key.SUPER, key.T}, actions = {} },
    -- { keys = {key.SUPER, key.R}, actions = {"reload_config"} },
  },

  format_window_title = function(ctx)
    return ctx.session .. " - " .. ctx.title
  end,
  left_status = function(ctx) return "" end,
  right_status = function(ctx) return "" end,
  format_tab = function(tab)
    return {{text = " " .. tab.title .. " ", color = tab.active and "#ffffff" or "#888888"}}
  end,
}
