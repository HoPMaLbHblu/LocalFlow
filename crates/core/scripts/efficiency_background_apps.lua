-- Puts apps you're not using into efficiency mode, like Task Manager's leaf button:
-- they get low priority and run on efficient processor cores, so the app in front of
-- you stays fast and the laptop runs cooler. The app you switch to is put back to normal.
-- Runs every 5 minutes. Change the list to the apps you keep open in the background.
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Efficiency mode for background apps",

    run = function(ctx)
        local apps = { "Discord", "Telegram", "Spotify", "steamwebhelper", "OneDrive" }

        local front = window.active()
        local front_app = front and front.app or ""
        for _, name in ipairs(apps) do
            if process.running(name) then
                local in_front = front_app:lower() == name:lower()
                local ok, e = pcall(process.set_efficiency, name, not in_front)
                if ok then
                    log(name .. (in_front and ": normal (you're using it)" or ": efficiency mode"))
                else
                    log(name .. ": " .. tostring(e))
                end
            end
        end
    end
}
