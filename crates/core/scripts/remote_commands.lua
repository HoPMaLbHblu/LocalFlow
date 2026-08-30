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

-- "dota.lookup: no hero called ..." -> "no hero called ..."
local function reason(e)
    local text = tostring(e)
    return text:match("dota%.[%w_]+: ([^\n]*)") or text
end

local function clock(seconds)
    return string.format("%d:%02d", seconds // 60, seconds % 60)
end

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
        "Dota 2:",
        "/draft - the draft and 3 suggested heroes",
        "/build [hero] - item plan for your hero",
        "/counter <hero> - heroes strong against it",
        "/lastmatch - review of your last match",
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

elseif COMMAND == "/draft" then
    local d = dota.draft()
    local function names(slots)
        local list = {}
        for _, slot in ipairs(slots) do
            if slot.hero then list[#list + 1] = slot.hero .. (slot.uncertain and "?" or "") end
        end
        return #list > 0 and table.concat(list, ", ") or "-"
    end
    local lines = {
        "You: " .. (d.player_hero or "?") .. (d.role and (" (" .. d.role .. ")") or ""),
        "Team: " .. names(d.allies),
        "Enemies: " .. names(d.enemies),
    }
    local ok, list = pcall(dota.suggest, 3)
    if ok and #list > 0 then
        lines[#lines + 1] = "Picks:"
        for i, s in ipairs(list) do
            lines[#lines + 1] = i .. ". " .. s.hero .. (s.reasons[1] and (" - " .. s.reasons[1].text) or "")
        end
    elseif not ok then
        lines[#lines + 1] = "No suggestions: " .. reason(list)
    end
    reply(table.concat(lines, "\n"))

elseif COMMAND == "/build" then
    local ok, plan = pcall(dota.build, ARGS ~= "" and ARGS or nil)
    if not ok then
        reply("No item plan: " .. reason(plan) .. (ARGS == "" and "\nOr name a hero: /build Axe" or ""))
        return
    end
    local function names(list, count)
        local out = {}
        for i, item in ipairs(list) do
            if i > count then break end
            out[#out + 1] = item.item
        end
        return table.concat(out, ", ")
    end
    local lines = { plan.hero .. " items" }
    if #plan.starting > 0 then lines[#lines + 1] = "Start: " .. names(plan.starting, 6) end
    if #plan.core > 0 then lines[#lines + 1] = "Core: " .. names(plan.core, 6) end
    if #plan.situational > 0 then lines[#lines + 1] = "Maybe: " .. names(plan.situational, 4) end
    for i, change in ipairs(plan.adaptations) do
        if i > 2 then break end
        lines[#lines + 1] = "- " .. change.text
    end
    reply(table.concat(lines, "\n"))

elseif COMMAND == "/counter" then
    if ARGS == "" then reply("Which hero? For example: /counter Anti-Mage") return end
    local ok, info = pcall(dota.lookup, ARGS, 5)
    if not ok then reply(reason(info)) return end
    local lines = { "Counters to " .. info.hero .. ":" }
    for i, m in ipairs(info.weak_against) do
        if i > 5 then break end
        lines[#lines + 1] = i .. ". " .. m.hero .. " - " .. m.reason.text
    end
    if #info.weak_against == 0 then lines[#lines + 1] = "no data yet" end
    reply(table.concat(lines, "\n"))

elseif COMMAND == "/lastmatch" then
    local ok, r = pcall(dota.last_match)
    if not ok then reply(reason(r)) return end
    local s = r.summary
    local lines = {
        string.format("%s %s, %d/%d/%d", s.hero, s.won and "won" or "lost", s.kills, s.deaths, s.assists),
        string.format("GPM %d, XPM %d, %d last hits, %s", s.gpm, s.xpm, s.last_hits, clock(s.duration_secs)),
    }
    for i, b in ipairs(r.benchmarks) do
        if i > 3 then break end
        if b.percentile then
            lines[#lines + 1] = string.format("%s: better than %d%%", b.metric, math.floor(b.percentile * 100 + 0.5))
        end
    end
    for i, note in ipairs(r.notes) do
        if i > 2 then break end
        lines[#lines + 1] = "- " .. note.text
    end
    reply(table.concat(lines, "\n"))

else
    reply("I don't know " .. COMMAND .. ". Send /help for the list.")
end
