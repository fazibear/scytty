return {
  format_window_title = function(ctx) return ctx.session .. " - " .. ctx.title end,
  left_status = function() return "" end,
  right_status = function() return "" end,
  format_tab = function(tab)
    return {{text = " " .. tab.title .. " ", color = tab.active and "#ffffff" or "#888888"}}
  end,
}
