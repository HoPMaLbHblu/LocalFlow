-- Turns the sound down at night and back up in the morning.
-- Runs at 8:00 and 22:00.
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Quiet hours",

    run = function(ctx)
        local night_volume, day_volume = 15, 50
        local hour = time.date().hour

        if hour >= 22 or hour < 8 then
            store.set("day_volume", system.volume())   -- remember your daytime level
            system.set_volume(night_volume)
            log("Night volume: " .. night_volume)
        else
            local volume = store.get("day_volume", day_volume)
            system.set_volume(volume)
            log("Day volume: " .. volume)
        end
    end
}
