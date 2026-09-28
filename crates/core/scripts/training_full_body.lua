-- A full-body strength program for beginners: Monday, Wednesday and Friday at 18:00,
-- 8 weeks, a little harder every week and a lighter week every 4th week.
-- You get a heads-up 30 minutes before, the exact exercises at 18:00, a nudge at 21:00 if
-- nothing was logged, and a summary every week. Log a workout with Ctrl+Alt+W
-- (the "I finished my workout" template). Change the start date, days and exercises below.

local plan = require("lf.plan")

local program = plan.new({
    name = "Full body",
    start = time.format("%Y-%m-%d"),   -- starts today; or a date like "2026-10-05"
    weeks = 8,
    at = "18:00",
    deload = 4,
    days = { mon = "A", wed = "B", fri = "A" },
    workouts = {
        A = { title = "Full body A", exercises = {
            { "Squats", sets = 3, reps = 10, add = 1, max = 15 },
            { "Push-ups", sets = 3, reps = 6, add = 1, max = 15 },
            { "Glute bridges", sets = 3, reps = 12, add = 1, max = 20 },
            { "Plank", sets = 3, seconds = 20, add = 5, max = 60 },
        }},
        B = { title = "Full body B", exercises = {
            { "Lunges (each leg)", sets = 3, reps = 8, add = 1, max = 12 },
            { "Rows with a backpack", sets = 3, reps = 10, add = 1, max = 15 },
            { "Pike push-ups", sets = 3, reps = 5, add = 1, max = 12 },
            { "Side plank (each side)", sets = 2, seconds = 20, add = 5, max = 45 },
        }},
    },
})

automation {
    name = "Training: full body",

    run = function(ctx)
        -- Also send to Telegram or read aloud: plan.remind(program, { telegram = true, speak = true })
        plan.remind(program)
    end
}
