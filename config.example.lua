-- Copy to ~/.config/scytty/config.lua (or $XDG_CONFIG_HOME/scytty/config.lua).
return {
  font_size = 16,
  scrollback_mb = 16, -- Per-tab MiB, 0–64; 0 disables scrollback.
  font_family = "monospace", -- Or an installed family name.
  icon_scale = 1.4, -- Nerd Font icons followed by a space (1–3).
  background_opacity = 0.8,
  window_width = 960,
  window_height = 640,

  format_window_title = function(ctx)
    return ctx.session .. " - " .. ctx.title
  end,
  left_status = function(ctx) return "" end,
  right_status = function(ctx) return "" end,
  format_tab = function(tab)
    return {{text = " " .. tab.title .. " ", color = tab.active and "#ffffff" or "#888888"}}
  end,
}
