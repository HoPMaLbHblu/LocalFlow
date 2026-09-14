-- Sets a Windows wake timer, so the PC wakes up from SLEEP at 7:30.
-- Runs every evening at 22:00; put the PC to sleep (not shut down) at night.
--
-- Good to know:
--   * No program can switch on a PC that is fully shut down; only the BIOS can.
--   * Windows must allow wake timers: Control Panel > Power Options >
--     Change plan settings > Change advanced power settings > Sleep >
--     Allow wake timers > Enable.
--   * To do things after waking, schedule another automation for 07:31,
--     for example "Open my work apps".
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Wake my PC every morning",

    run = function(ctx)
        local when = system.wake_at("07:30")
        log("The PC will wake up at " .. when)
    end
}
