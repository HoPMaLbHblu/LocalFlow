-- Warns you when one program keeps the processor busy for a long time
-- (a stuck app, a runaway browser tab). Runs every 5 minutes; it warns
-- only when the same program was busy at two checks in a row.

automation {
    name = "Busy program alert",

    run = function(ctx)
        local limit = 50   -- percent of the whole processor

        local top = process.top(1, "cpu")[1]
        if not top then return end
        log(string.format("Busiest: %s at %.0f%%", top.name, top.cpu))

        if top.cpu >= limit then
            if store.get("busy", "") == top.name then
                notify(string.format("%s has been using %.0f%% of the processor for a while", top.name, top.cpu))
            end
            store.set("busy", top.name)
        else
            store.set("busy", "")
        end
    end
}
