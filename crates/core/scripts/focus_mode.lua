-- Press Ctrl+Alt+F to close distracting apps and mute the sound.
-- Windows are closed the polite way (like clicking X), so apps can still
-- ask you to save. Change the list to your own distractions.
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Focus mode",

    run = function(ctx)
        local distracting = { discord = true, steam = true, telegram = true }
        local closed = 0

        for _, w in ipairs(window.list()) do
            if distracting[w.app] then
                window.close(w)
                closed = closed + 1
                log("Closed " .. w.title)
            end
        end

        system.mute()
        notify("Focus mode on: closed " .. closed .. " window(s)")
    end
}
