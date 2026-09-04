-- Open the apps you use every day with one click.
--
-- Tip: tick "Run when LocalFlow starts" and turn on Settings > Start with Windows,
-- and your apps open by themselves when you sign in. You can also run this
-- from the tray icon: right-click > Run > Open my work apps.

automation {
    name = "Open my work apps",

    run = function(ctx)
        -- Use the names from your Start menu. Not sure of a name?
        -- Create the "List my apps" template and run it.
        local apps = {
            "notepad",
            -- "Spotify",
            -- "Discord",
            -- "C:/Program Files/Some App/app.exe",
        }

        for _, name in ipairs(apps) do
            if app.running(name) then
                log(name .. " is already open")
            else
                app.open(name)
                log("Opened " .. name)
                wait(1) -- give each app a moment to start
            end
        end

        -- Websites work too:
        -- app.open("https://calendar.google.com")
    end
}
