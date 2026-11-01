--!nonstrict
-- Bar widget: renders the devices the monitor service publishes, an icon and
-- level for each.

local battery = require("./battery.luau")

local status = noctalia.state.get("status")

-- The device's own glyph, or its "-off" form while it can't be read.
local function deviceGlyph(s, off)
  local name = s.kind == "keyboard" and "keyboard" or "mouse"
  return off and (name .. "-off") or name
end

local function glyphFor(s)
  if s.state == "error" then
    return deviceGlyph(s, true)
  elseif s.charging or noctalia.getConfig("icon") == "battery" then
    return battery.glyph(s.battery, s.charging)
  end
  return deviceGlyph(s, false)
end

local function textFor(s)
  if s.battery ~= nil and noctalia.getConfig("show_percent") then
    return tostring(s.battery) .. "%"
  end
  return ""
end

local function colorFor(s)
  if s.state == "error" or s.low then
    return "error"
  elseif s.state ~= "connected" then
    -- Asleep: the level shown is the last one read.
    return "outline"
  end
  return "on_surface"
end

local function level(s)
  if s.state == "error" then
    return s.error or ""
  end
  return s.battery ~= nil and battery.value(s) or noctalia.tr("battery.offline")
end

local function deviceName(s)
  return s.device or noctalia.tr("tooltip." .. (s.kind or "mouse"))
end

-- Key/value rows like Noctalia's built-in widgets, with the shared battery
-- wording. One device: battery then device. Several: a row per device keyed
-- by its kind, since Noctalia caps tooltips at 280 px and cuts long names short
-- (a name like "ROG Falchion RX Low Profile" doesn't fit as a key or value).
local function tooltip(devices)
  if #devices == 1 then
    local s = devices[1]
    if s.state == "error" then
      return { { noctalia.tr("tooltip.device"), deviceName(s) }, { noctalia.tr("tooltip.error"), level(s) } }
    end
    return { { noctalia.tr("tooltip.battery"), level(s) }, { noctalia.tr("tooltip.device"), deviceName(s) } }
  end
  local rows = {}
  for _, s in ipairs(devices) do
    table.insert(rows, { noctalia.tr("tooltip." .. (s.kind or "mouse")), level(s) })
  end
  return rows
end

-- Several devices need a declarative row; a Noctalia without
-- `barWidget.render` shows the first one's icon and every level.
local function renderDevices(devices)
  if barWidget.render ~= nil then
    local pairs_ = {}
    for _, s in ipairs(devices) do
      local color = colorFor(s)
      local children = { ui.glyph({ name = glyphFor(s), color = color }) }
      local text = textFor(s)
      if text ~= "" then
        table.insert(children, ui.label({ text = text, color = color }))
      end
      table.insert(pairs_, ui.row({ gap = 4, align = "center" }, children))
    end
    local container = barWidget.isVertical() and ui.column or ui.row
    barWidget.render(container({ gap = 8, align = "center" }, pairs_))
    return
  end
  local first = devices[1]
  local texts = {}
  for _, s in ipairs(devices) do
    local text = textFor(s)
    if text ~= "" then
      table.insert(texts, text)
    end
  end
  barWidget.setGlyph(glyphFor(first))
  barWidget.setText(table.concat(texts, " "))
  barWidget.setColor(colorFor(first))
  barWidget.setGlyphColor(colorFor(first))
end

-- An icon and text alone: while waiting, on an error and with nothing found.
-- Once `barWidget.render` has drawn a tree, the glyph and text don't show, so
-- these draw a tree too where it's available.
local function renderSingle(glyph, text, color)
  if barWidget.render ~= nil then
    local children = { ui.glyph({ name = glyph, color = color }) }
    if text ~= "" then
      table.insert(children, ui.label({ text = text, color = color }))
    end
    barWidget.render(ui.row({ gap = 4, align = "center" }, children))
    return
  end
  barWidget.setGlyph(glyph)
  barWidget.setText(text)
  barWidget.setColor(color)
  barWidget.setGlyphColor(color)
end

local function render()
  local s = status
  if s == nil then
    renderSingle("mouse", "", "on_surface")
    barWidget.setTooltip(noctalia.tr("tooltip.waiting"))
    return
  end

  -- The service couldn't run rogctl.
  if s.error ~= nil then
    barWidget.setVisible(true)
    renderSingle("mouse-off", "", "error")
    barWidget.setTooltip({ { noctalia.tr("tooltip.error"), s.error } })
    return
  end

  local devices = s.devices or {}
  if #devices == 0 then
    if noctalia.getConfig("hide_when_disconnected") then
      barWidget.setVisible(false)
      return
    end
    barWidget.setVisible(true)
    renderSingle("mouse-off", "", "outline")
    barWidget.setTooltip({
      { noctalia.tr("tooltip.battery"), noctalia.tr("battery.unknown") },
      { noctalia.tr("tooltip.device"), noctalia.tr("tooltip.not_connected") },
    })
    return
  end

  barWidget.setVisible(true)
  renderDevices(devices)
  barWidget.setTooltip(tooltip(devices))
end

noctalia.state.watch("status", function(value)
  status = value
  render()
end)

-- Ask the service to read the devices right away.
function onClick()
  noctalia.state.set("refresh", noctalia.nowMs())
end

render()
