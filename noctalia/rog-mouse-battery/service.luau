--!nonstrict
-- Runs `rogctl watch --json` and publishes each status line to shared state
-- under "status" for the bar widget. Also sends the low-battery notification,
-- here rather than in the widget so it fires once no matter how many bars
-- show the widget.

local battery = require("./battery.luau")

local STATUS_KEY = "status"
local REFRESH_KEY = "refresh"

local lastLineAt = 0
local lastStatus = nil
local lowNotified = false

local function shellQuote(value)
  return "'" .. tostring(value):gsub("'", "'\\''") .. "'"
end

-- Always a whole number in the manifest's range: it goes into a shell command,
-- since runStream takes no argv array.
local function interval()
  local secs = math.floor(tonumber(noctalia.getConfig("interval")) or 60)
  return math.clamp(secs, 5, 600)
end

local function publish(status)
  status.received = os.time()
  lastStatus = status
  noctalia.state.set(STATUS_KEY, status)
end

local function publishError(message)
  publish({ state = "error", charging = false, error = message })
end

local function checkLowBattery(status)
  local threshold = noctalia.getConfig("low_threshold") or 0
  local level = status.battery
  if threshold <= 0 or level == nil or status.state ~= "connected" then
    return
  end
  if status.charging or level > threshold + 5 then
    lowNotified = false
  elseif level <= threshold and not lowNotified then
    lowNotified = true
    -- Solaar's format: the device name as the title, its battery line as the body.
    noctalia.notify(status.device or noctalia.tr("tooltip.mouse"), battery.line(status))
  end
end

-- rogctl keeps the last level while the mouse sleeps, but a one-shot read
-- (click to refresh) starts fresh, so carry the level over here too.
local function mergeAsleep(status)
  if status.state == "asleep" and status.battery == nil and lastStatus ~= nil and lastStatus.device == status.device then
    status.battery = lastStatus.battery
    status.low_battery_warning = lastStatus.low_battery_warning
    status.power_off_minutes = lastStatus.power_off_minutes
    status.battery_updated = lastStatus.battery_updated
  end
  return status
end

local function handleLine(line)
  lastLineAt = os.time()
  local status, err = noctalia.json.decode(line)
  if type(status) ~= "table" then
    noctalia.log("rog-mouse-battery: bad line from rogctl: " .. tostring(err))
    return
  end
  publish(mergeAsleep(status))
  checkLowBattery(status)
end

local function startStream()
  local binary = noctalia.getConfig("binary") or "rogctl"
  if not noctalia.commandExists(binary) and not binary:find("/") then
    publishError(noctalia.tr("error.missing_binary", { binary = binary }))
    return
  end
  lastLineAt = os.time()
  local cmd = shellQuote(binary) .. " watch --json --interval " .. tostring(interval())
  if not noctalia.runStream(cmd, handleLine) then
    publishError(noctalia.tr("error.launch_failed", { binary = binary }))
  end
end

local function refreshNow()
  local binary = noctalia.getConfig("binary") or "rogctl"
  noctalia.runAsync({ binary, "battery", "--json" }, function(result)
    local line = noctalia.string.trim(result.stdout or "")
    if line ~= "" then
      local status = noctalia.json.decode(line)
      if type(status) == "table" then
        publish(mergeAsleep(status))
      end
    end
  end)
end

noctalia.state.watch(REFRESH_KEY, function()
  refreshNow()
end)

-- rogctl prints a line every interval, so a long silence means it exited
-- (e.g. the binary was missing or crashed). Start it again.
function update()
  noctalia.setUpdateInterval(10000)
  if os.time() - lastLineAt > interval() * 3 + 10 then
    startStream()
  end
end

startStream()
