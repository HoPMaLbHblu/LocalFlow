-- lf.plan: training programs (or any weekly routine) with reminders, progression and a log.
--
--   local plan = require("lf.plan")
--   local program = plan.new({
--       name = "Full body",
--       start = "2026-10-05",          -- the first day of week 1
--       weeks = 8,                     -- leave out to keep going
--       at = "18:00",                  -- default time of a session
--       remind_before = 30,            -- minutes; a heads-up before each session (0 = none)
--       days = { mon = "A", wed = "B", fri = { workout = "A", at = "10:00" } },
--       deload = 4,                    -- every 4th week is lighter (70 %); leave out for none
--       workouts = {
--           A = { title = "Legs and push", exercises = {
--               { "Squats", sets = 3, reps = 10, add = 1, max = 20 },   -- +1 rep a week, at most 20
--               { "Plank", sets = 3, seconds = 30, add = 5 },            -- +5 s a week
--               { "Run", by_week = { "20 min easy", "25 min easy", "2 x 12 min" } },  -- one entry per week
--           }},
--       },
--   })
--   program:session(time.now())        -- today's session (numbers for this week) or nil
--   plan.remind(program)               -- in an automation that runs every few minutes
--   plan.done(program)                 -- in a hotkey automation: "I finished my workout"
--
-- Times are in the PC's local time. Nothing here needs system control.

local plan = {}

local DAY_NAMES = { "mon", "tue", "wed", "thu", "fri", "sat", "sun" }
local DAY_TITLES = { "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday" }
local DEFAULT_LOG = "~/Documents/LocalFlow/Training log.csv"

local Program = {}
Program.__index = Program

-- ---- time helpers ---------------------------------------------------------------------------

--- Midnight of the day that contains timestamp `t`.
local function day_start(t)
    local d = time.date(t)
    return time.make({ year = d.year, month = d.month, day = d.day })
end

--- Midnight `n` days after the day of `t` (calendar days, safe across clock changes).
local function add_days(t, n)
    local d = time.date(t)
    return time.make({ year = d.year, month = d.month, day = d.day + n })
end

--- "18:30" -> 18, 30
local function parse_clock(text, what)
    local h, m = tostring(text or ""):match("^%s*(%d%d?):(%d%d)%s*$")
    h, m = tonumber(h), tonumber(m)
    if not h or h > 23 or m > 59 then
        error("lf.plan: " .. what .. " must be a time like \"18:30\", not \"" .. tostring(text) .. "\"", 3)
    end
    return h, m
end

local function at_time(day, clock)
    local h, m = parse_clock(clock, "a session time")
    local d = time.date(day)
    return time.make({ year = d.year, month = d.month, day = d.day, hour = h, min = m })
end

-- ---- building a program -------------------------------------------------------------------------

--- Check a program description and return a program. Mistakes are reported clearly.
function plan.new(def)
    if type(def) ~= "table" then
        error("lf.plan: plan.new needs a table describing the program", 2)
    end
    local p = setmetatable({}, Program)
    p.name = def.name or "Training"
    p.start = def.start and time.parse(def.start) or day_start(time.now())
    if not p.start then
        error("lf.plan: start must be a date like \"2026-10-05\", not \"" .. tostring(def.start) .. "\"", 2)
    end
    p.start = day_start(p.start)
    p.weeks = def.weeks
    p.at = def.at or "18:00"
    parse_clock(p.at, "at")
    p.remind_before = def.remind_before or 30
    p.deload = def.deload
    p.deload_factor = def.deload_factor or 0.7
    p.workouts = def.workouts or {}
    p.days = {}
    for key, value in pairs(def.days or {}) do
        local index
        for i, name in ipairs(DAY_NAMES) do
            if name == tostring(key):lower():sub(1, 3) then index = i end
        end
        if not index then
            error("lf.plan: unknown day \"" .. tostring(key) .. "\" (use mon, tue, wed, thu, fri, sat, sun, or daily)", 2)
        end
        local entry = type(value) == "table" and value or { workout = value }
        if not p.workouts[entry.workout] then
            error("lf.plan: " .. DAY_NAMES[index] .. " uses workout \"" .. tostring(entry.workout) .. "\", which isn't in workouts", 2)
        end
        entry.at = entry.at or p.at
        parse_clock(entry.at, DAY_NAMES[index] .. ".at")
        p.days[index] = entry
    end
    if def.daily then
        local entry = type(def.daily) == "table" and def.daily or { workout = def.daily }
        if not p.workouts[entry.workout] then
            error("lf.plan: daily uses workout \"" .. tostring(entry.workout) .. "\", which isn't in workouts", 2)
        end
        entry.at = entry.at or p.at
        for i = 1, 7 do
            if not p.days[i] then p.days[i] = entry end
        end
    end
    if next(p.days) == nil then
        error("lf.plan: the program has no training days (add days = { mon = \"A\", ... })", 2)
    end
    return p
end

--- Week number (1, 2, ...) for timestamp `t`; 0 before the start.
function Program:week(t)
    local s = day_start(t or time.now())
    if s < self.start then return 0 end
    local days = 0
    local d = self.start
    -- Count calendar days (not seconds) so clock changes don't shift weeks.
    while d < s and days < 100000 do
        d = add_days(d, 1)
        days = days + 1
    end
    return days // 7 + 1
end

--- Whether the program is over at `t` (only when it has a number of weeks).
function Program:finished(t)
    return self.weeks ~= nil and self:week(t or time.now()) > self.weeks
end

function Program:is_deload(week)
    return self.deload ~= nil and self.deload > 1 and week % self.deload == 0
end

--- One exercise's target for a week, as text: "3 x 12", "3 x 40 s", "25 min", or the by_week text.
local function target(ex, week, deload, factor)
    if ex.by_week then
        local v = ex.by_week[math.min(week, #ex.by_week)]
        return tostring(v)
    end
    local function grow(base)
        if not base then return nil end
        local v = base + (ex.add or 0) * (week - 1)
        if ex.max then v = math.min(v, ex.max) end
        if deload then v = math.max(1, math.floor(v * factor + 0.5)) end
        return math.floor(v + 0.5)
    end
    local sets = ex.sets
    local reps, seconds, minutes, km = grow(ex.reps), grow(ex.seconds), grow(ex.minutes), ex.km and (ex.km + (ex.add_km or 0) * (week - 1))
    local amount
    if reps then amount = reps
    elseif seconds then amount = seconds .. " s"
    elseif minutes then amount = minutes .. " min"
    elseif km then amount = string.format("%g km", deload and km * factor or km)
    end
    if sets and amount then return sets .. " x " .. amount end
    return tostring(amount or ex.note or "")
end

--- The session on the day of timestamp `t`, or nil (rest day, before the start, or finished).
--- Returns { program, workout, title, at (timestamp), week, deload, exercises = { {name, target, note}, ... } }.
function Program:session(t)
    t = t or time.now()
    local week = self:week(t)
    if week == 0 or self:finished(t) then return nil end
    local entry = self.days[time.date(t).weekday]
    if not entry then return nil end
    local w = self.workouts[entry.workout]
    local deload = self:is_deload(week)
    local exercises = {}
    for _, ex in ipairs(w.exercises or {}) do
        exercises[#exercises + 1] = { name = ex[1] or ex.name or "?", target = target(ex, week, deload, self.deload_factor), note = ex.note }
    end
    return {
        program = self.name,
        workout = entry.workout,
        title = w.title or entry.workout,
        note = w.note,
        at = at_time(day_start(t), entry.at),
        week = week,
        deload = deload,
        exercises = exercises,
    }
end

--- The next `count` sessions from `from` (default now), for a preview.
function Program:upcoming(count, from)
    local out = {}
    local day = day_start(from or time.now())
    for _ = 1, 400 do
        if #out >= (count or 5) or self:finished(day) then break end
        local s = self:session(day)
        if s and s.at >= (from or 0) then out[#out + 1] = s end
        day = add_days(day, 1)
    end
    return out
end

--- A session as readable text for a notification.
function plan.describe(s)
    local lines = { string.format("%s - %s (week %d%s)", s.program, s.title, s.week, s.deload and ", lighter week" or "") }
    for _, ex in ipairs(s.exercises) do
        lines[#lines + 1] = "- " .. ex.name .. (ex.target ~= "" and (": " .. ex.target) or "") .. (ex.note and (" (" .. ex.note .. ")") or "")
    end
    if s.note then lines[#lines + 1] = s.note end
    return table.concat(lines, "\n")
end

--- Reminders due in (`from`, `to`]: { { kind = "soon" | "start", at = timestamp, session = ... }, ... }.
--- Looks back at most 3 hours, so a PC that was off doesn't get a pile of old reminders.
function Program:due(from, to)
    to = to or time.now()
    from = math.max(from or (to - 600), to - 3 * 3600)
    local out = {}
    local day = day_start(from)
    while day <= to do
        local s = self:session(day)
        if s then
            local soon = s.at - self.remind_before * 60
            if self.remind_before > 0 and soon > from and soon <= to then
                out[#out + 1] = { kind = "soon", at = soon, session = s }
            end
            if s.at > from and s.at <= to then
                out[#out + 1] = { kind = "start", at = s.at, session = s }
            end
        end
        day = add_days(day, 1)
    end
    table.sort(out, function(a, b) return a.at < b.at end)
    return out
end

-- ---- the training log (a CSV file you can open in Excel) -------------------------------------------

local function quote(v)
    v = tostring(v or "")
    if v:find('[,"\n]') then v = '"' .. v:gsub('"', '""') .. '"' end
    return v
end

--- Days (YYYY-MM-DD) with a "done" entry in the log, for one program (or all when nil).
function plan.done_days(program_name, log_path)
    local path = log_path or DEFAULT_LOG
    local days = {}
    if not fs.exists(path) then return days end
    for _, row in ipairs(csv.read(path, { header = true })) do
        if row.status == "done" and (program_name == nil or row.program == program_name or row.program == "") then
            days[row.date] = true
        end
    end
    return days
end

--- Write "I did it" into the log for today (once per day and program). Returns the text shown.
--- `program` may be a program from plan.new (rest days then don't break the streak) or a name.
function plan.done(program, options)
    options = options or {}
    local program_name = type(program) == "table" and program.name or program
    if type(program) == "table" then options.program = program end
    local path = options.log or DEFAULT_LOG
    local today = time.format("%Y-%m-%d")
    if plan.done_days(program_name, path)[today] then
        return "Already logged for today. Well done!"
    end
    if not fs.exists(path) then
        fs.write(path, "date,time,program,status,note\n")
    end
    fs.append(path, table.concat({ today, time.format("%H:%M"), quote(program_name or ""), "done", quote(options.note or "") }, ",") .. "\n")
    -- A streak: days in a row with a logged workout (rest days don't break it when a program is given).
    local days = plan.done_days(program_name, path)
    local streak, day = 0, day_start(time.now())
    for _ = 1, 3650 do
        if days[time.format("%Y-%m-%d", day)] then
            streak = streak + 1
        elseif not (options.program and options.program:session(day) == nil) then
            break
        end
        day = add_days(day, -1)
    end
    return string.format("Workout logged. Streak: %d", streak)
end

-- ---- running it from an automation ---------------------------------------------------------------------

local function say(text, options)
    notify(text)
    log(text)
    if options.speak then pcall(speak, (text:gsub("\n.*", ""))) end
    if options.telegram and telegram and telegram.available() then pcall(telegram.send, text) end
    if options.discord and discord and discord.available() then pcall(discord.send, text) end
end

--- Everything an "every few minutes" automation needs: heads-up and start reminders, an evening
--- nudge when nothing is logged, a weekly summary, and a message when the program is complete.
--- options: log (CSV path), done_hotkey (text shown), evening ("21:00" or false),
---          speak, telegram, discord (also send there), now (for tests).
function plan.remind(program, options)
    options = options or {}
    local now = options.now or time.now()
    local hotkey = options.done_hotkey or "Ctrl+Alt+W"
    local key = "plan:" .. program.name
    local last = store.get(key .. ":last", now - 600)
    local shown = 0

    if program:finished(now) then
        if not store.get(key .. ":complete", false) then
            say("Program complete: " .. program.name .. ". Great work!", options)
            store.set(key .. ":complete", true)
        end
        store.set(key .. ":last", now)
        return 1
    end

    for _, event in ipairs(program:due(last, now)) do
        local s = event.session
        if event.kind == "soon" then
            say(string.format("In %d min: %s\n%s", program.remind_before, s.title, plan.describe(s)), options)
        else
            say("Time to train!\n" .. plan.describe(s) .. "\nWhen you finish, press " .. hotkey .. ".", options)
        end
        shown = shown + 1
    end

    -- Evening nudge: today's session isn't in the log yet.
    local evening = options.evening == nil and "21:00" or options.evening
    local today_s = program:session(now)
    local today = time.format("%Y-%m-%d", now)
    if evening and today_s and now >= at_time(day_start(now), evening) and last < at_time(day_start(now), evening)
        and not plan.done_days(program.name, options.log)[today] then
        say("Did you train today? " .. today_s.title .. " isn't logged yet. Press " .. hotkey .. " if you did.", options)
        shown = shown + 1
    end

    -- Weekly summary when a new week starts.
    local week = program:week(now)
    local seen = store.get(key .. ":week", week)
    if week > seen and seen >= 1 then
        local planned, done = 0, 0
        local days = plan.done_days(program.name, options.log)
        local day = add_days(program.start, (seen - 1) * 7)
        for _ = 1, 7 do
            if program:session(day) then
                planned = planned + 1
                if days[time.format("%Y-%m-%d", day)] then done = done + 1 end
            end
            day = add_days(day, 1)
        end
        say(string.format("Week %d of %s: %d of %d workouts done.%s", seen, program.name, done, planned,
            done >= planned and " Perfect week!" or ""), options)
        shown = shown + 1
    end
    store.set(key .. ":week", week)
    store.set(key .. ":last", now)
    return shown
end

return plan
