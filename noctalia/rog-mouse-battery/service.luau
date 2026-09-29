--!nonstrict
-- Runs `rogctl watch --json` and publishes each status line to shared state
-- under "status" for the bar widget. Also sends the low-battery notification
-- when rogctl flags one (`notify_low`), here rather than in the widget so it
-- fires once no matter how many bars show the widget.

local battery = require("./battery.luau")

local STATUS_KEY = "status"
local REFRESH_KEY = "refresh"
-- The stream's shell prints this, then rogctl's exit status, when rogctl exits.
local EXIT_MARKER = "rog-mouse-battery:exited "
-- How long to wait before starting rogctl again after it exits on its own.
local RESTART_SECONDS = 10

local streaming = false
local lastStatus = nil
-- The settings the running rogctl was started with.
local streamSettings = nil
-- Set while rogctl is being stopped to apply new settings.
local restarting = false
local exitedAt = 0

local function shellQuote(value)
  return "'" .. tostring(value):gsub("'", "'\\''") .. "'"
end

-- Always whole numbers in the manifest's ranges: they go into a shell command,
-- since runStream takes no argv array.
local function interval()
  local secs = math.floor(tonumber(noctalia.getConfig("interval")) or 60)
  return math.clamp(secs, 5, 600)
end

local function lowThreshold()
  local pct = math.floor(tonumber(noctalia.getConfig("low_threshold")) or 20)
  return math.clamp(pct, 0, 100)
end

local function binaryName()
  return noctalia.getConfig("binary") or "rogctl"
end

local function currentSettings()
  return binaryName() .. "\n" .. tostring(interval()) .. "\n" .. tostring(lowThreshold())
end

local function publish(status)
  status.received = os.time()
  lastStatus = status
  noctalia.state.set(STATUS_KEY, status)
end

local function publishError(message)
  publish({ state = "error", charging = false, error = message })
end

-- Forward declaration: handleLine restarts the stream, startStream reads lines.
local startStream

local function handleLine(line)
  if line:sub(1, #EXIT_MARKER) == EXIT_MARKER then
    streaming = false
    if restarting then
      restarting = false
      startStream()
      return
    end
    exitedAt = os.time()
    publishError(noctalia.tr("error.exited", { binary = binaryName(), code = line:sub(#EXIT_MARKER + 1) }))
    return
  end
  local status, err = noctalia.json.decode(line)
  if type(status) ~= "table" then
    noctalia.log("rog-mouse-battery: bad line from rogctl: " .. tostring(err))
    return
  end
  publish(status)
  if status.notify_low then
    -- Solaar's format: the device name as the title, its battery line as the body.
    noctalia.notify(status.device or noctalia.tr("tooltip.mouse"), battery.line(status))
  end
end

function startStream()
  local binary = binaryName()
  exitedAt = os.time()
  streamSettings = currentSettings()
  if not noctalia.commandExists(binary) and not binary:find("/") then
    publishError(noctalia.tr("error.missing_binary", { binary = binary }))
    return
  end
  local script = shellQuote(binary)
    .. " watch --json --interval "
    .. tostring(interval())
    .. " --low-threshold "
    .. tostring(lowThreshold())
    .. "; echo "
    .. shellQuote(EXIT_MARKER)
    .. '"$?"'
  -- Noctalia runs streams as `sh -c "exec <cmd>"`, which would replace the
  -- shell with rogctl and skip the echo, so the script gets its own shell.
  streaming = noctalia.runStream("sh -c " .. shellQuote(script), handleLine)
  if not streaming then
    publishError(noctalia.tr("error.launch_failed", { binary = binary }))
  end
end

-- SIGUSR1 makes the running rogctl read the mouse now; it reports its pid in
-- each line since runStream doesn't.
local function refreshNow()
  local pid = lastStatus and lastStatus.pid
  if streaming and pid ~= nil then
    noctalia.runAsync({ "kill", "-USR1", tostring(pid) })
  end
end

noctalia.state.watch(REFRESH_KEY, function()
  refreshNow()
end)

-- Start rogctl again once the exit marker says it has stopped. Silence alone
-- isn't enough: rogctl's sleep pauses across suspend, so restarting a quiet one
-- would leave two running.
--
-- Noctalia has no settings-changed event and can't stop a stream, so this also
-- polls the settings, and when they change stops rogctl itself (it reports its
-- pid); the exit marker then starts it again with the new ones.
function update()
  noctalia.setUpdateInterval(2000)
  if not streaming then
    if os.time() - exitedAt >= RESTART_SECONDS then
      startStream()
    end
  elseif not restarting and currentSettings() ~= streamSettings then
    local pid = lastStatus and lastStatus.pid
    if pid ~= nil then
      restarting = true
      noctalia.runAsync({ "kill", tostring(pid) })
    end
  end
end

startStream()
