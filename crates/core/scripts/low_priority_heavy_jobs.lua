-- When a heavy program starts (video converter, archiver, game compiler...), it gets
-- "below normal" priority, so the rest of the PC stays responsive while it works.
-- The trigger is "When this app starts: HandBrake"; change it to your program.
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Heavy jobs at low priority",

    run = function(ctx)
        local name = ctx.app or "HandBrake"
        wait(3) -- give it a moment to start its worker processes
        local count = process.set_priority(name, "below_normal")
        log(string.format("%s: %d process(es) set to below normal", name, count))
    end
}
