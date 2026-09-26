-- Opens your page (for example your Dotabuff profile) when Dota 2 reaches its main menu,
-- once per launch of the game. Restarting LocalFlow during the same game doesn't open it again.
--
-- Set it up in Settings › Dota 2 companion:
--   * "Page to open": your page, e.g. https://www.dotabuff.com/players/<your id>
--     (leave it empty to open nothing).
--   * For the exact moment the menu appears, install Game State Integration there too
--     (Valve's official feature: the game tells LocalFlow its state, nothing reads its memory).
--
-- This automation starts when the dota2 program starts (see the trigger below). Scripts stop
-- at their time limit (Settings › Scripts); if the menu isn't up by then, the page opens anyway.
-- LocalFlow can also do this on its own without a time limit: switch on
-- "Open my page when Dota 2 starts" in Settings › Dota 2 companion.

automation {
    name = "Dota 2: open my page at launch",

    run = function(ctx)
        local ready = dota.wait_for_menu(600)
        if not ready then
            log("The menu wasn't reported before the time limit; opening the page now.")
        end
        local opened, why = dota.open_launch_url()
        if opened then
            log("Opened your Dota 2 page.")
        else
            log("Nothing opened: " .. why)
        end
    end
}
