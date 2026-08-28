-- Press Ctrl+Alt+W when you finish a workout. It goes into your training log
-- (Documents/LocalFlow/Training log.csv, opens in Excel), and the training reminders
-- use it for the evening nudge, weekly summaries and your streak.

local plan = require("lf.plan")

automation {
    name = "I finished my workout",

    run = function(ctx)
        notify(plan.done())
    end
}
