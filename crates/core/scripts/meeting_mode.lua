-- Press Ctrl+Alt+M before a video call: mutes notifications' sound, closes
-- chat apps that might pop up, and opens your meeting app. Press it again
-- afterwards to switch back (the chat apps are opened again).
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Meeting mode",

    run = function(ctx)
        local chat_apps = { "Discord", "Telegram", "WhatsApp" }   -- names from the Start menu
        local meeting_app = "Zoom"                                -- or "Microsoft Teams", ...

        local on = not store.get("on", false)
        if on then
            local closed = {}
            for _, name in ipairs(chat_apps) do
                if app.running(name) then
                    process.kill(name)
                    closed[#closed + 1] = name
                end
            end
            store.set("closed", closed)
            system.mute()
            if not app.running(meeting_app) then
                app.open(meeting_app)
            end
            notify("Meeting mode on. Closed: " .. (#closed > 0 and table.concat(closed, ", ") or "nothing"))
        else
            for _, name in ipairs(store.get("closed", {})) do
                app.open(name)
            end
            system.mute() -- the mute key toggles, so this turns sound back on
            notify("Meeting mode off. Welcome back!")
        end
        store.set("on", on)
    end
}
