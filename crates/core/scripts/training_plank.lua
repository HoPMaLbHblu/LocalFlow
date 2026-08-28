-- A 4-week plank challenge: every day at 12:30, a little longer each week.
-- Keep your body straight; stop a set early rather than let your hips drop.

local plan = require("lf.plan")

local program = plan.new({
    name = "Plank challenge",
    start = time.format("%Y-%m-%d"),
    weeks = 4,
    at = "12:30",
    remind_before = 0,
    daily = "P",
    workouts = {
        P = { title = "Plank", exercises = {
            { "Plank", sets = 3, seconds = 30, add = 15 },
            { "Side plank (each side)", sets = 1, seconds = 20, add = 10 },
        }},
    },
})

automation {
    name = "Training: plank challenge",

    run = function(ctx)
        -- Also send to Telegram or read aloud: plan.remind(program, { telegram = true, speak = true })
        plan.remind(program)
    end
}
