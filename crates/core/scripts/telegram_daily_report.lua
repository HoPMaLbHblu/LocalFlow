-- Every evening, sends a short report about your PC to your Telegram:
-- disk space, memory, battery and app updates.
-- Set up the Telegram bot in Settings first.

automation {
    name = "Daily PC report to Telegram",

    run = function(ctx)
        local gb = 1024 * 1024 * 1024
        local lines = { "PC report, " .. time.format("%d.%m.%Y") }
        for _, disk in ipairs(system.disks()) do
            if not disk.removable then
                lines[#lines + 1] = string.format("Disk %s: %.0f GB free", disk.mount, disk.free / gb)
            end
        end
        local memory = system.memory()
        lines[#lines + 1] = string.format("Memory in use: %.0f%%", memory.used / memory.total * 100)
        local battery = system.battery()
        if battery then lines[#lines + 1] = "Battery: " .. battery.percent .. "%" end
        local ok, updates = pcall(packages.updates)
        if ok and #updates > 0 then lines[#lines + 1] = #updates .. " app(s) can be updated" end

        telegram.send(table.concat(lines, "\n"))
        log("Sent")
    end
}
