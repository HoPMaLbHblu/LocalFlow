-- Posts a warning to your Discord channel when a disk is almost full or the
-- memory is nearly used up. Runs every 15 minutes and warns once per problem.
-- Set up the Discord webhook in Settings first.

automation {
    name = "PC alerts to Discord",

    run = function(ctx)
        local problems = {}
        for _, disk in ipairs(system.disks()) do
            local free_gb = disk.free / (1024 * 1024 * 1024)
            if not disk.removable and free_gb < 10 then
                problems["disk " .. disk.mount] = string.format("Disk %s has only %.1f GB free", disk.mount, free_gb)
            end
        end
        local memory = system.memory()
        if memory.used / memory.total > 0.9 then
            local top = process.top(1, "memory")[1]
            problems.memory = "Memory is over 90% full" .. (top and (", mostly " .. top.name) or "")
        end

        local warned = store.get("warned", {})
        for key, text in pairs(problems) do
            if not warned[key] then
                discord.send(system.computer_name() .. ": " .. text)
                log(text)
            end
        end
        store.set("warned", problems)   -- a problem that goes away can warn again later
    end
}
