-- Press Ctrl+Alt+P to see which programs use the most memory right now.

automation {
    name = "What is using my memory?",

    run = function(ctx)
        local memory = system.memory()
        local lines = { string.format("Memory used: %.0f%%", memory.used / memory.total * 100) }
        for _, p in ipairs(process.top(5, "memory")) do
            lines[#lines + 1] = string.format("%s: %.1f GB", p.name, p.memory_mb / 1024)
        end
        log(table.concat(lines, "\n"))
        notify(table.concat(lines, "\n"))
    end
}
