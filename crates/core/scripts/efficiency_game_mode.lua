-- Press Ctrl+Alt+G before a game: chat, launchers and sync apps go into efficiency mode,
-- so the game gets the processor. Press again afterwards to put them back to normal.
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Game mode: calm background apps",

    run = function(ctx)
        local apps = { "Discord", "Telegram", "Spotify", "steamwebhelper", "EpicGamesLauncher", "OneDrive", "chrome", "msedge" }

        local on = not store.get("on", false)
        local changed = {}
        for _, name in ipairs(apps) do
            if process.running(name) and pcall(process.set_efficiency, name, on) then
                changed[#changed + 1] = name
            end
        end
        store.set("on", on)
        notify((on and "Game mode on. Calmed: " or "Game mode off. Back to normal: ")
            .. (#changed > 0 and table.concat(changed, ", ") or "nothing was running"))
    end
}
