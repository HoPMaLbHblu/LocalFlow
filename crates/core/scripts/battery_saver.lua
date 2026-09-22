-- On a laptop: switches to the power saver plan when the battery is low and you
-- are not plugged in, and back to your usual plan when you plug in.
-- Runs every 5 minutes.
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Battery saver",

    run = function(ctx)
        local low = 25

        local battery = system.battery()
        if not battery then
            log("This PC has no battery")
            return
        end

        local saving = store.get("saving", false)
        if not battery.plugged_in and battery.percent <= low and not saving then
            store.set("usual_plan", power.plan())
            power.set_plan("power saver")
            store.set("saving", true)
            notify("Battery at " .. battery.percent .. "%. Switched to power saver.")
        elseif battery.plugged_in and saving then
            power.set_plan(store.get("usual_plan", "balanced"))
            store.set("saving", false)
            notify("Plugged in. Back to your usual power plan.")
        end
        log("Battery " .. battery.percent .. "%" .. (battery.plugged_in and ", plugged in" or ""))
    end
}
