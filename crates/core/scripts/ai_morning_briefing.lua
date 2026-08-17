-- Every morning, GigaChat turns your PC's state into a short, friendly note:
-- free disk space, memory, app updates, and how long the PC has been on.
-- Only these numbers are sent, nothing else. Set up GigaChat in Settings first.

automation {
    name = "Morning PC briefing",

    run = function(ctx)
        local gb = 1024 * 1024 * 1024
        local facts = {}
        for _, disk in ipairs(system.disks()) do
            if not disk.removable then
                facts[#facts + 1] = string.format("disk %s: %.0f GB free of %.0f GB", disk.mount, disk.free / gb, disk.total / gb)
            end
        end
        local memory = system.memory()
        facts[#facts + 1] = string.format("memory in use: %.0f%%", memory.used / memory.total * 100)
        facts[#facts + 1] = string.format("on for %.1f days", system.uptime() / 86400)
        local ok, updates = pcall(packages.updates)
        if ok then facts[#facts + 1] = #updates .. " apps have updates" end

        local text = table.concat(facts, "\n")
        if ai.available() then
            text = ai.ask("Write a friendly good-morning note of at most 3 short sentences about this computer. Suggest one thing to do only if something needs attention.\n\n" .. text)
        end
        notify(text)
        log(text)
    end
}
