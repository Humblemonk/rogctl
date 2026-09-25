--!nonstrict
-- Bar widget: renders the status the monitor service publishes.

local battery = require("./battery.luau")

local status = noctalia.state.get("status")

-- Key/value rows like Noctalia's built-in widgets, primary value first,
-- with Solaar's battery wording.
local function tooltip(s)
  local device = s.device or noctalia.tr("tooltip.not_connected")
  if s.state == "error" then
    return { { noctalia.tr("tooltip.device"), device }, { noctalia.tr("tooltip.error"), s.error or "" } }
  end
  local level = s.battery ~= nil and battery.value(s) or noctalia.tr("battery.offline")
  if s.state == "disconnected" then
    level = noctalia.tr("battery.unknown")
  end
  return { { noctalia.tr("tooltip.battery"), level }, { noctalia.tr("tooltip.device"), device } }
end

local function render()
  local s = status
  if s == nil then
    barWidget.setGlyph("mouse")
    barWidget.setText("")
    barWidget.setTooltip(noctalia.tr("tooltip.waiting"))
    return
  end

  if s.state == "disconnected" and noctalia.getConfig("hide_when_disconnected") then
    barWidget.setVisible(false)
    return
  end
  barWidget.setVisible(true)

  local glyph
  if s.state == "disconnected" or s.state == "error" then
    glyph = "mouse-off"
  elseif s.charging or noctalia.getConfig("icon") == "battery" then
    glyph = battery.glyph(s.battery, s.charging)
  else
    glyph = "mouse"
  end
  barWidget.setGlyph(glyph)

  local text = ""
  if s.battery ~= nil and noctalia.getConfig("show_percent") then
    text = tostring(s.battery) .. "%"
  end
  barWidget.setText(text)

  local threshold = noctalia.getConfig("low_threshold") or 0
  local low = s.battery ~= nil and not s.charging and threshold > 0 and s.battery <= threshold
  local color
  if s.state == "error" or low then
    color = "error"
  elseif s.state ~= "connected" then
    -- Asleep or gone: the level shown is the last one read.
    color = "outline"
  else
    color = "on_surface"
  end
  barWidget.setColor(color)
  barWidget.setGlyphColor(color)

  barWidget.setTooltip(tooltip(s))
end

noctalia.state.watch("status", function(value)
  status = value
  render()
end)

-- Ask the service to read the mouse right away.
function onClick()
  noctalia.state.set("refresh", noctalia.nowMs())
end

render()
