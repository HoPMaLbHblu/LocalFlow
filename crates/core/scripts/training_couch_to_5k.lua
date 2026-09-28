-- Couch to 5K: from walking to running 5 km in 9 weeks. Three runs a week (Monday,
-- Wednesday, Saturday). Each run starts with a 5-minute brisk walk. Listen to your body:
-- repeat a week whenever you need to. Log a run with Ctrl+Alt+W.

local plan = require("lf.plan")

local program = plan.new({
    name = "Couch to 5K",
    start = time.format("%Y-%m-%d"),
    weeks = 9,
    at = "07:30",
    days = { mon = "R", wed = "R", sat = { workout = "R", at = "10:00" } },
    workouts = {
        R = { title = "Run", note = "Warm up with a 5-minute brisk walk, cool down with 5 minutes of walking.", exercises = {
            { "Intervals", by_week = {
                "8 x (60 s jog, 90 s walk)",
                "6 x (90 s jog, 2 min walk)",
                "2 x (90 s jog, 90 s walk, 3 min jog, 3 min walk)",
                "3 min jog, 90 s walk, 5 min jog, 2.5 min walk, 3 min jog, 90 s walk, 5 min jog",
                "5 min jog, 3 min walk, 5 min jog, 3 min walk, 5 min jog",
                "5 min jog, 3 min walk, 8 min jog, 3 min walk, 5 min jog",
                "25 min jog",
                "28 min jog",
                "30 min jog - that's about 5 km!",
            }},
        }},
    },
})

automation {
    name = "Training: Couch to 5K",

    run = function(ctx)
        -- Also send to Telegram or read aloud: plan.remind(program, { telegram = true, speak = true })
        plan.remind(program)
    end
}
