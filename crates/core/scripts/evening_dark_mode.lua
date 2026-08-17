-- Dark mode for apps in the evening, light mode in the morning.
-- Runs at 8:00 and 20:00. Only the apps change; your taskbar stays as it is
-- (use "system" instead of "apps" to switch the taskbar too).
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Evening dark mode",

    run = function(ctx)
        local hour = time.date().hour
        local dark = hour >= 20 or hour < 8
        desktop.set_dark_mode(dark, "apps")
        log(dark and "Dark mode on" or "Light mode on")
    end
}
