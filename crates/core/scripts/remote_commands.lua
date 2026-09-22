-- The commands of the Telegram remote control (Settings › Telegram and Discord).
-- LocalFlow runs this for every message from YOUR chat, with:
--   COMMAND      "/volume"          ARGS  "20"
--   ALLOW_POWER  true when "Allow shutdown, restart and closing apps" is on
-- /run and /list are handled by LocalFlow itself.

local function reply(text) telegram.send(text) end

local power_commands = { ["/close"] = true, ["/sleep"] = true, ["/shutdown"] = true, ["/restart"] = true }
if power_commands[COMMAND] and not ALLOW_POWER then
    reply("That command is switched off. Turn on \"Allow shutdown, restart and closing apps\" in LocalFlow's settings.")
    return
end

local gb = 1024 * 1024 * 1024

if COMMAND == "/start" or COMMAND == "/help" or COMMAND == "" then
    reply(table.concat({
        "LocalFlow remote control",
        "",
        "/status - how the PC is doing",
        "/screenshot - a picture of the screen",
        "/top - the busiest programs",
        "/apps - open windows",
        "/open <app> - open an app",
        "/lock - lock the PC",
        "/volume <0-100> - set the volume (without a number: show it)",
        "/mute, /unmute",
        "/say <text> - show and read a message aloud on the PC",
        "/clipboard - what is copied on the PC",
        "/list - your automations",
        "/run <automation> - run one of your automations",
        "",
        "With \"Allow shutdown...\" on:",
        "/close <app>, /sleep, /shutdown, /restart, /cancel",
    }, "\n"))

elseif COMMAND == "/status" then
    local lines = { system.computer_name() .. " is on" }
    local memory = system.memory()
    lines[#lines + 1] = string.format("Processor: %.0f%%   Memory: %.0f%%", system.cpu(), memory.used / memory.total * 100)
    for _, disk in ipairs(system.disks()) do
        lines[#lines + 1] = string.format("Disk %s: %.0f GB free of %.0f", disk.mount, disk.free / gb, disk.total / gb)
    end
    local battery = system.battery()
    if battery then
        lines[#lines + 1] = "Battery: " .. battery.percent .. "%" .. (battery.plugged_in and " (plugged in)" or "")
    end
    lines[#lines + 1] = string.format("Up for %.1f hours, idle for %d min", system.uptime() / 3600, system.idle_seconds() // 60)
    reply(table.concat(lines, "\n"))

elseif COMMAND == "/screenshot" then
    -- Kept in Pictures/Screenshots/Telegram, so you can see what was sent.
    local shot = screen.capture("~/Pictures/Screenshots/Telegram/" .. time.format("%Y-%m-%d %H-%M-%S") .. ".jpg")
    telegram.send_photo(shot, time.format("%H:%M:%S"))

elseif COMMAND == "/top" then
    local lines = {}
    for _, p in ipairs(process.top(8, "cpu")) do
        lines[#lines + 1] = string.format("%s: %.0f%% CPU, %.0f MB", p.name, p.cpu, p.memory_mb)
    end
    reply(table.concat(lines, "\n"))

elseif COMMAND == "/apps" then
    local lines = {}
    for _, w in ipairs(window.list()) do
        lines[#lines + 1] = w.app .. ": " .. w.title
    end
    reply(#lines > 0 and table.concat(lines, "\n") or "No open windows")

elseif COMMAND == "/open" then
    if ARGS == "" then reply("Which app? For example: /open Notepad") return end
    app.open(ARGS)
    reply("Opened " .. ARGS)

elseif COMMAND == "/close" then
    if ARGS == "" then reply("Which app? For example: /close Notepad") return end
    app.close(ARGS)
    reply("Asked " .. ARGS .. " to close")

elseif COMMAND == "/lock" then
    system.lock()
    reply("Locked")

elseif COMMAND == "/volume" then
    local level = tonumber(ARGS)
    if level then
        system.set_volume(math.max(0, math.min(100, level)))
    end
    reply("Volume: " .. system.volume() .. (system.muted() and " (muted)" or ""))

elseif COMMAND == "/mute" or COMMAND == "/unmute" then
    system.set_mute(COMMAND == "/mute")
    reply(COMMAND == "/mute" and "Muted" or "Sound on")

elseif COMMAND == "/say" then
    if ARGS == "" then reply("What should I say? For example: /say Dinner is ready") return end
    speak(ARGS)   -- LocalFlow also shows every command as a notification
    reply("Said it")

elseif COMMAND == "/clipboard" then
    local text = clipboard.get() or ""
    reply(text ~= "" and text or "The clipboard is empty")

elseif COMMAND == "/sleep" then
    reply("Going to sleep")
    system.sleep()

elseif COMMAND == "/shutdown" or COMMAND == "/restart" then
    local what = COMMAND == "/shutdown" and "Shutting down" or "Restarting"
    if COMMAND == "/shutdown" then system.shutdown(60) else system.restart(60) end
    reply(what .. " in 1 minute. Send /cancel to stop it.")

elseif COMMAND == "/cancel" then
    system.cancel_shutdown()
    reply("Cancelled")

else
    reply("I don't know " .. COMMAND .. ". Send /help for the list.")
end
