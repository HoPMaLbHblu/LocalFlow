-- Tests for lf.plan. TEST_DIR is an empty folder scripts may use.
local test = require("lf.test")
local plan = require("lf.plan")

local function at(text) return time.parse(text) end

local program = plan.new({
    name = "Test program",
    start = "2026-10-05", -- a Monday
    weeks = 4,
    at = "18:00",
    remind_before = 30,
    deload = 4,
    days = { mon = "A", wed = "B", fri = { workout = "A", at = "10:00" } },
    workouts = {
        A = { title = "Workout A", exercises = {
            { "Squats", sets = 3, reps = 10, add = 2, max = 14 },
            { "Plank", sets = 2, seconds = 30, add = 10 },
        }},
        B = { title = "Run", exercises = { { "Run", by_week = { "20 min easy", "25 min easy", "2 x 12 min" } } } },
    },
})

test.group("plan: weeks and days", function()
    test.case("week numbers", function()
        test.equal(program:week(at("2026-10-04 12:00")), 0)
        test.equal(program:week(at("2026-10-05 00:00")), 1)
        test.equal(program:week(at("2026-10-11 23:59")), 1)
        test.equal(program:week(at("2026-10-12 08:00")), 2)
        test.equal(program:finished(at("2026-11-01 12:00")), false)
        test.equal(program:finished(at("2026-11-02 12:00")), true)
    end)
    test.case("sessions only on training days", function()
        test.equal(program:session(at("2026-10-06 12:00")), nil) -- Tuesday: rest
        local mon = program:session(at("2026-10-05 07:00"))
        test.equal(mon.title, "Workout A")
        test.equal(time.format("%H:%M", mon.at), "18:00")
        local fri = program:session(at("2026-10-09 07:00"))
        test.equal(time.format("%H:%M", fri.at), "10:00")
        test.equal(program:session(at("2026-10-04 07:00")), nil) -- before the start
        test.equal(program:session(at("2026-11-02 07:00")), nil) -- after the end
    end)
end)

test.group("plan: progression", function()
    test.case("reps and seconds grow each week, up to max", function()
        local w1 = program:session(at("2026-10-05 07:00")).exercises
        test.equal(w1[1].target, "3 x 10")
        test.equal(w1[2].target, "2 x 30 s")
        local w3 = program:session(at("2026-10-19 07:00")).exercises
        test.equal(w3[1].target, "3 x 14")   -- 10 + 2*2
        test.equal(w3[2].target, "2 x 50 s")
    end)
    test.case("every 4th week is lighter", function()
        local s = program:session(at("2026-10-26 07:00"))
        test.equal(s.deload, true)
        test.equal(s.exercises[1].target, "3 x 10")  -- min(16, 14) * 0.7 = 9.8 -> 10
        test.equal(s.exercises[2].target, "2 x 42 s") -- 60 * 0.7
        test.truthy(plan.describe(s):find("lighter week", 1, true))
    end)
    test.case("by_week text, and the last entry repeats", function()
        test.equal(program:session(at("2026-10-07 07:00")).exercises[1].target, "20 min easy")
        test.equal(program:session(at("2026-10-28 07:00")).exercises[1].target, "2 x 12 min")
    end)
    test.case("upcoming sessions", function()
        local next3 = program:upcoming(3, at("2026-10-05 19:00"))
        test.equal(#next3, 3)
        test.equal(time.format("%a", next3[1].at), "Wed")
    end)
end)

test.group("plan: reminders", function()
    test.case("heads-up and start in the window", function()
        local events = program:due(at("2026-10-05 17:25"), at("2026-10-05 18:05"))
        test.equal(#events, 2)
        test.equal(events[1].kind, "soon")
        test.equal(events[2].kind, "start")
        test.equal(#program:due(at("2026-10-05 18:05"), at("2026-10-05 18:15")), 0)
    end)
    test.case("a PC that was off for days gets no pile of old reminders", function()
        test.equal(#program:due(at("2026-10-01 00:00"), at("2026-10-09 23:00")), 0)
    end)
    test.case("remind() shows each reminder once", function()
        local log_file = TEST_DIR .. "/log1.csv"
        test.equal(plan.remind(program, { log = log_file, now = at("2026-10-05 17:20"), evening = false }), 0)
        test.equal(plan.remind(program, { log = log_file, now = at("2026-10-05 17:35"), evening = false }), 1)
        test.equal(plan.remind(program, { log = log_file, now = at("2026-10-05 18:01"), evening = false }), 1)
        test.equal(plan.remind(program, { log = log_file, now = at("2026-10-05 18:10"), evening = false }), 0)
    end)
end)

test.group("plan: log", function()
    test.case("done is written once a day and read back", function()
        local log_file = TEST_DIR .. "/log2.csv"
        local first = plan.done(program, { log = log_file })
        test.truthy(first:find("logged", 1, true))
        test.truthy(plan.done(program, { log = log_file }):find("Already", 1, true))
        test.truthy(plan.done_days("Test program", log_file)[time.format("%Y-%m-%d")])
        local text = fs.read(log_file)
        test.truthy(text:find("^date,time,program,status,note\n"))
    end)
end)

test.group("plan: streak", function()
    test.case("entries from before the program started don't count", function()
        local log_file = TEST_DIR .. "/log3.csv"
        local yesterday = time.format("%Y-%m-%d", time.now() - 86400)
        fs.write(log_file, "date,time,program,status,note\n" .. yesterday .. ",18:00,Fresh,done,\n")
        local fresh = plan.new({ name = "Fresh", start = time.format("%Y-%m-%d"), daily = "A",
            workouts = { A = { title = "A", exercises = { { "Squats", reps = 5 } } } } })
        test.equal(plan.done(fresh, { log = log_file }), "Workout logged. Streak: 1")
    end)
    test.case("notes with line breaks stay in one row", function()
        local log_file = TEST_DIR .. "/log4.csv"
        plan.done("Notes", { log = log_file, note = "line one\r\nline two, done" })
        local rows = csv.read(log_file, { header = true })
        test.equal(#rows, 1)
        test.equal(rows[1].note, "line one\r\nline two, done")
    end)
end)

test.group("plan: mistakes are explained", function()
    test.case("unknown workout, day or time", function()
        local ok, e = pcall(plan.new, { days = { mon = "X" }, workouts = {} })
        test.equal(ok, false)
        test.truthy(tostring(e):find("isn't in workouts", 1, true))
        ok, e = pcall(plan.new, { days = { someday = "A" }, workouts = { A = {} } })
        test.truthy(tostring(e):find("unknown day", 1, true))
        ok, e = pcall(plan.new, { at = "25:99", days = { mon = "A" }, workouts = { A = {} } })
        test.truthy(tostring(e):find("time like", 1, true))
        ok, e = pcall(plan.new, { workouts = { A = {} } })
        test.truthy(tostring(e):find("no training days", 1, true))
    end)
    test.case("daily programs", function()
        local p = plan.new({ start = "2026-10-05", daily = "S", workouts = { S = { title = "Stretch", exercises = { { "Hamstrings", seconds = 30 } } } } })
        test.equal(p:session(at("2026-10-10 07:00")).title, "Stretch")
        test.equal(p:session(at("2026-10-10 07:00")).exercises[1].target, "30 s")
    end)
end)

test.finish()
