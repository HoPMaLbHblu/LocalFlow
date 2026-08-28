-- Ten minutes of stretching every morning at 7:30. Hold each stretch calmly, don't bounce.
-- Log it with Ctrl+Alt+W to keep your streak.

local plan = require("lf.plan")

local program = plan.new({
    name = "Morning stretch",
    start = time.format("%Y-%m-%d"),
    at = "07:30",
    remind_before = 0,
    daily = "S",
    workouts = {
        S = { title = "Morning stretch", exercises = {
            { "Neck rolls", seconds = 30 },
            { "Shoulder circles", seconds = 30 },
            { "Cat-cow", seconds = 60 },
            { "Hamstring stretch (each leg)", seconds = 45 },
            { "Hip flexor lunge (each side)", seconds = 45 },
            { "Child's pose", seconds = 60 },
        }},
    },
})

automation {
    name = "Training: morning stretch",

    run = function(ctx)
        -- Also send to Telegram or read aloud: plan.remind(program, { telegram = true, speak = true })
        plan.remind(program)
    end
}
