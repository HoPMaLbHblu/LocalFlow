-- A 4-day split for the gym or home with weights: upper body Monday and Thursday,
-- lower body Tuesday and Friday, 12 weeks, a lighter week every 4th week.
-- Numbers are reps; pick a weight you can lift with 1-2 reps to spare. Log with Ctrl+Alt+W.

local plan = require("lf.plan")

local program = plan.new({
    name = "Upper / lower",
    start = time.format("%Y-%m-%d"),
    weeks = 12,
    at = "18:30",
    deload = 4,
    days = { mon = "U", tue = "L", thu = "U", fri = "L" },
    workouts = {
        U = { title = "Upper body", exercises = {
            { "Bench press or push-ups", sets = 3, reps = 8, add = 1, max = 12 },
            { "Rows", sets = 3, reps = 8, add = 1, max = 12 },
            { "Overhead press", sets = 3, reps = 8, add = 1, max = 12 },
            { "Pull-ups or pulldowns", sets = 3, reps = 5, add = 1, max = 10 },
        }},
        L = { title = "Lower body", exercises = {
            { "Squats", sets = 3, reps = 8, add = 1, max = 12 },
            { "Romanian deadlifts", sets = 3, reps = 8, add = 1, max = 12 },
            { "Lunges (each leg)", sets = 3, reps = 8, add = 1, max = 12 },
            { "Calf raises", sets = 3, reps = 12, add = 2, max = 20 },
        }},
    },
})

automation {
    name = "Training: upper / lower split",

    run = function(ctx)
        -- Also send to Telegram or read aloud: plan.remind(program, { telegram = true, speak = true })
        plan.remind(program)
    end
}
