-- Every evening, saves the day's system history (CPU, memory, disk, battery,
-- one row per minute) as a spreadsheet, plus the day's averages and peaks.
-- Handy to see when your PC is busiest, or how fast the battery drains.

local paths = require("lf.paths")
local dates = require("lf.dates")

automation {
    name = "Daily system history",

    run = function(ctx)
        local samples = metrics.recent(24 * 60)
        if #samples == 0 then
            log("No history yet.")
            return
        end

        local rows = {}
        for _, s in ipairs(samples) do
            rows[#rows + 1] = { time.format("%H:%M", s.at), s.cpu, s.memory, s.disk, s.battery or "" }
        end
        local folder = "~/Documents/LocalFlow reports/System"
        local file = paths.join(folder, "system " .. time.today() .. ".csv")
        csv.write(file, rows, { header = { "time", "cpu %", "memory %", "disk %", "battery %" } })

        local line = {}
        for _, name in ipairs({ "cpu", "memory" }) do
            line[#line + 1] = string.format("%s avg %.0f%% (peak %.0f%%)", name, metrics.average(name, 1440), metrics.peak(name, 1440))
        end
        local lowest = metrics.lowest("battery", 1440)
        if lowest then
            line[#line + 1] = string.format("battery lowest %.0f%%", lowest)
        end
        log(table.concat(line, ", "))
        log("Saved " .. #rows .. " rows (" .. dates.duration(#rows * 60) .. ") to " .. file)
    end
}
