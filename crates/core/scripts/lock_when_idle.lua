-- Locks the PC when nobody has used the keyboard or mouse for 10 minutes.
-- Change the minutes under "More triggers and permissions".
--
-- Needs "Allow system control".

automation {
    name = "Lock when I walk away",

    run = function(ctx)
        if ctx.trigger == "test" then
            log("Test run: would lock the PC now (idle for " .. system.idle_seconds() .. " seconds)")
            return
        end
        system.lock()
    end
}
