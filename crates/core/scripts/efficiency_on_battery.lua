-- On a laptop: when you unplug, busy background apps go into efficiency mode to save battery;
-- when you plug in again, they go back to normal. Runs every 5 minutes.
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Efficiency mode on battery",

    run = function(ctx)
        local apps = { "chrome", "msedge", "Discord", "Telegram", "Spotify", "OneDrive" }

        local battery = system.battery()
        if not battery then
            log("This PC has no battery")
            return
        end
        local saving = not battery.plugged_in
        if store.get("saving", false) == saving then
            return -- nothing changed since the last check
        end
        for _, name in ipairs(apps) do
            if process.running(name) then
                pcall(process.set_efficiency, name, saving)
            end
        end
        store.set("saving", saving)
        notify(saving and "On battery: background apps are in efficiency mode" or "Plugged in: apps are back to normal")
    end
}
