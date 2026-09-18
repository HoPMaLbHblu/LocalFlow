-- Checks every 10 minutes that a website is up, and tells you when it goes
-- down and when it's back. Tries three times before deciding it's down, so a
-- short hiccup doesn't cause a false alarm.

local retry = require("lf.retry")
local dates = require("lf.dates")

automation {
    name = "Is my website up?",

    run = function(ctx)
        local url = "https://example.com"   -- change to your site

        local ok, result = retry.try(function()
            return retry.times(3, function()
                local r = http.get(url)
                if not r.ok then
                    error("status " .. r.status, 0)
                end
                return r
            end, { wait = 5 })
        end)

        local was_up = store.get("up", true)
        if ok then
            log(url .. " is up")
            if not was_up then
                local down_for = time.now() - store.get("down_since", time.now())
                notify(url .. " is back up (it was down for " .. dates.duration(down_for) .. ")")
            end
            store.set("up", true)
        else
            log(url .. " is down: " .. result)
            if was_up then
                store.set("down_since", time.now())
                notify(url .. " is DOWN: " .. result)
            end
            store.set("up", false)
        end
    end
}
