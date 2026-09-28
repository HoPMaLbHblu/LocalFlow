-- Press Ctrl+Alt+L to start studying: opens your "Study" tabs and puts distracting apps
-- into efficiency mode. Press again when you're done to put them back.
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Study session",

    run = function(ctx)
        local distracting = { "Discord", "Telegram", "steamwebhelper" }

        local on = not store.get("on", false)
        if on then
            if not links.get("Study") then
                links.save("Study", "https://en.wikipedia.org\nhttps://translate.google.com")
            end
            links.open("Study")
        end
        for _, name in ipairs(distracting) do
            if process.running(name) then pcall(process.set_efficiency, name, on) end
        end
        store.set("on", on)
        notify(on and "Study session started. Good luck!" or "Study session over. Apps are back to normal.")
    end
}
