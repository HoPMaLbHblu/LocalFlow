-- Checks the PC every 15 minutes using LocalFlow's system history, and warns
-- when the processor or memory has been busy for a long time (often a program
-- stuck in the background), when the disk is nearly full, or the battery low.
-- Each warning is shown at most once every few hours.

local retry = require("lf.retry")

automation {
    name = "PC health check",

    run = function(ctx)
        local minutes = 30
        local cpu = metrics.average("cpu", minutes)
        if not cpu then
            log("No history yet; LocalFlow records it once a minute while it runs.")
            return
        end
        local memory = metrics.average("memory", minutes)
        local latest = metrics.latest()
        log(string.format("Last %d min: CPU %.0f%%, memory %.0f%%, disk %.0f%% full", minutes, cpu, memory, latest.disk))

        local warnings = {
            { cpu > 85, "cpu", string.format("The processor has been %.0f%% busy for %d minutes. Check Task Manager for a stuck program.", cpu, minutes) },
            { memory > 90, "memory", string.format("Memory has been %.0f%% full for %d minutes. Closing some apps will speed things up.", memory, minutes) },
            { latest.disk > 95, "disk", string.format("The Windows disk is %.0f%% full.", latest.disk) },
            { latest.battery and latest.battery < 15, "battery", "Battery is at " .. tostring(latest.battery) .. "%." },
        }
        for _, w in ipairs(warnings) do
            if w[1] and retry.at_most_every("pc-health-" .. w[2], time.hours(3)) then
                notify(w[3])
            end
        end
    end
}
