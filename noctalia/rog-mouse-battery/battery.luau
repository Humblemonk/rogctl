--!nonstrict
-- Battery wording and icons shared by the widget and service, following
-- Solaar (lib/logitech_receiver/common.py, lib/solaar/ui/icons.py).

local M = {}

-- Solaar's status words: discharging, recharging, full.
function M.statusWord(s)
  if s.charging and s.battery == 100 then
    return noctalia.tr("battery.full")
  elseif s.charging then
    return noctalia.tr("battery.recharging")
  end
  return noctalia.tr("battery.discharging")
end

-- "79% (discharging)"; "(offline)" replaces the status while the mouse sleeps.
function M.value(s)
  local word = s.state == "connected" and M.statusWord(s) or noctalia.tr("battery.offline")
  return noctalia.tr("battery.value", { percent = s.battery, status = word })
end

-- "Battery: 79% (discharging)", Solaar's notification body.
function M.line(s)
  return noctalia.tr("battery.line", { percent = s.battery, status = M.statusWord(s) })
end

-- Solaar's semantic levels: full >= 90, good >= 60, low >= 20, caution >= 5, empty.
function M.glyph(percent, charging)
  if charging then
    return "battery-charging"
  elseif percent == nil then
    return "battery-exclamation"
  elseif percent >= 90 then
    return "battery-4"
  elseif percent >= 60 then
    return "battery-3"
  elseif percent >= 20 then
    return "battery-2"
  elseif percent >= 5 then
    return "battery-1"
  end
  return "battery"
end

return M
