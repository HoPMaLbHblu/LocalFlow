-- Press Ctrl+Alt+A: your browser goes on the left half of the screen,
-- your editor on the right half. Change the app names to the ones you use.
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Arrange my windows",

    run = function(ctx)
        local width, height = screen.size()
        local half = width // 2
        local usable = height - 48 -- leave room for the taskbar

        local left = window.find("chrome") or window.find("msedge") or window.find("firefox")
        local right = window.find("code") or window.find("notepad")

        if left then
            window.move(left, 0, 0, half, usable)
            log("Left: " .. left.title)
        end
        if right then
            window.move(right, half, 0, half, usable)
            log("Right: " .. right.title)
        end
        if not left and not right then
            log("None of the apps are open. Edit the names in the script.")
        end
    end
}
