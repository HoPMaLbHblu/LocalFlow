-- Checks that your sites, NAS, printer or game server answer, and warns you
-- when one stops answering (and again when it's back). Runs every 5 minutes.

automation {
    name = "Server watch",

    run = function(ctx)
        -- name, address, port (443 = websites, 22 = SSH, 445 = shared folders, 631 = printers)
        local servers = {
            { "Example site", "example.com", 443 },
            { "Home router", "192.168.1.1", 80 },
        }

        for _, s in ipairs(servers) do
            local name, host, port = s[1], s[2], s[3]
            local ms = network.ping(host, port)
            local was_up = store.get(name, true)
            if ms then
                log(string.format("%s answers in %.0f ms", name, ms))
            else
                log(name .. " does not answer")
            end
            if (ms ~= nil) ~= was_up then
                notify(ms and (name .. " is back") or (name .. " is not answering"))
            end
            store.set(name, ms ~= nil)
        end
    end
}
