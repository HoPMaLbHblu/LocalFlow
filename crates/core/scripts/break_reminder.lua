-- Every hour, if you've been at the PC, reminds you to stand up,
-- stretch and look away from the screen.

automation {
    name = "Take a break",

    run = function(ctx)
        if system.idle_seconds() > 5 * 60 then
            log("You are already away")
            return
        end
        notify("Time for a short break: stand up, stretch, and look at something far away.")
        speak("Time for a short break")
    end
}
