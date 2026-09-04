-- Shows the names app.open() understands (your Start menu),
-- and the programs that are running right now.

automation {
    name = "List my apps",

    run = function(ctx)
        local shortcuts = app.shortcuts()
        log("Apps in your Start menu (" .. #shortcuts .. "):")
        for _, name in ipairs(shortcuts) do
            log("  " .. name)
        end

        local running = app.list()
        log("Running right now (" .. #running .. "):")
        log("  " .. table.concat(running, ", "))
    end
}
