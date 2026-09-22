-- Tells you when the internet goes down and when it comes back,
-- and how long it was gone. Runs every minute.

automation {
    name = "Internet watch",

    run = function(ctx)
        local online = network.online()
        local was_online = store.get("online", true)

        if not online and was_online then
            store.set("down_since", time.now())
            notify("The internet is down")
        elseif online and not was_online then
            local minutes = math.floor((time.now() - store.get("down_since", time.now())) / 60)
            notify("The internet is back after " .. minutes .. " min")
            log("Outage: " .. minutes .. " min")
        end
        store.set("online", online)
        log(online and "Online" or "Offline")
    end
}
