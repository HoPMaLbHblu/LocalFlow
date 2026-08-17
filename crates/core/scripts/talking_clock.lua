-- Says the time out loud every hour, from 9:00 to 21:00.
-- Nice when you lose track of time.

automation {
    name = "Talking clock",

    run = function(ctx)
        local hour = time.date().hour
        if hour < 9 or hour > 21 then return end
        speak("It's " .. time.format("%H:%M"))
    end
}
