-- Build up to many push-ups in 6 weeks: Tuesday, Thursday and Saturday at 19:00.
-- Rest a minute between sets. If a day is too hard, repeat last week's numbers.
-- Log a workout with Ctrl+Alt+W.

local plan = require("lf.plan")

local program = plan.new({
    name = "Push-up challenge",
    start = time.format("%Y-%m-%d"),
    weeks = 6,
    at = "19:00",
    days = { tue = "P", thu = "P", sat = "P" },
    workouts = {
        P = { title = "Push-ups", exercises = {
            { "Push-ups", by_week = { "5 sets: 6, 8, 6, 6, as many as you can",
                                      "5 sets: 9, 11, 8, 8, as many as you can",
                                      "5 sets: 12, 17, 13, 13, as many as you can",
                                      "5 sets: 16, 20, 16, 16, as many as you can",
                                      "5 sets: 20, 25, 20, 20, as many as you can",
                                      "5 sets: 25, 30, 25, 25, as many as you can" } },
            { "Plank", sets = 2, seconds = 30, add = 10 },
        }},
    },
})

automation {
    name = "Training: push-up challenge",

    run = function(ctx)
        -- Also send to Telegram or read aloud: plan.remind(program, { telegram = true, speak = true })
        plan.remind(program)
    end
}
