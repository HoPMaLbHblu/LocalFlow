-- lf.dates: dates, weekdays and durations.
--
--   local dates = require("lf.dates")
--   dates.add_days(time.now(), 7)           -- a timestamp one week from now
--   dates.days_between("2026-01-01", time.today())
--   dates.duration(3725)                    --> "1 h 2 min 5 s"
--   dates.ago(fs.modified(file))            --> "3 days ago"
--
-- Functions accept a timestamp (a number) or a date as text ("2026-09-29" or
-- "2026-09-29 14:05"). They return timestamps unless the name says otherwise.

local dates = {}

local DAY = 24 * 60 * 60

--- Turn a timestamp or date text into a timestamp. nil means now.
function dates.stamp(value)
    if value == nil then
        return time.now()
    end
    if type(value) == "number" then
        return value
    end
    local stamp = time.parse(value)
    if not stamp then
        error("dates: \"" .. tostring(value) .. "\" is not a date (use \"2026-09-29\" or \"2026-09-29 14:05\")", 3)
    end
    return stamp
end

--- A timestamp from year, month, day (and optional hour, minute, second).
function dates.make(year, month, day, hour, min, sec)
    return time.make({ year = year, month = month, day = day, hour = hour, min = min, sec = sec })
end

--- Move a date by whole days; keeps the time of day, also across clock changes.
function dates.add_days(value, days)
    local d = time.date(dates.stamp(value))
    return time.make({ year = d.year, month = d.month, day = d.day + days, hour = d.hour, min = d.min, sec = d.sec })
        or dates.stamp(value) + days * DAY
end

local function days_in_month(year, month)
    local lengths = { 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31 }
    if month == 2 and dates.is_leap_year(year) then
        return 29
    end
    return lengths[month]
end

--- Move a date by whole months. Jan 31 + 1 month = Feb 28 (or 29).
function dates.add_months(value, months)
    local d = time.date(dates.stamp(value))
    local index = d.year * 12 + (d.month - 1) + months
    local year, month = index // 12, index % 12 + 1
    local day = math.min(d.day, days_in_month(year, month))
    return time.make({ year = year, month = month, day = day, hour = d.hour, min = d.min, sec = d.sec })
end

function dates.is_leap_year(year)
    return (year % 4 == 0 and year % 100 ~= 0) or year % 400 == 0
end

function dates.days_in_month(year, month)
    return days_in_month(year, month)
end

--- Midnight at the start of the day.
function dates.start_of_day(value)
    local d = time.date(dates.stamp(value))
    return time.make({ year = d.year, month = d.month, day = d.day })
end

--- The last second of the day.
function dates.end_of_day(value)
    return dates.add_days(dates.start_of_day(value), 1) - 1
end

--- Monday 00:00 of the week.
function dates.start_of_week(value)
    local stamp = dates.start_of_day(value)
    return dates.add_days(stamp, -(time.date(stamp).weekday - 1))
end

--- The first day of the month, 00:00.
function dates.start_of_month(value)
    local d = time.date(dates.stamp(value))
    return time.make({ year = d.year, month = d.month, day = 1 })
end

--- Whole calendar days from `a` to `b` (negative if b is earlier).
function dates.days_between(a, b)
    local from, to = time.date(dates.stamp(a)), time.date(dates.stamp(b))
    -- Count at noon, so clock changes don't shift the result.
    local x = time.make({ year = from.year, month = from.month, day = from.day, hour = 12 })
    local y = time.make({ year = to.year, month = to.month, day = to.day, hour = 12 })
    return math.floor((y - x) / DAY + 0.5)
end

function dates.is_weekend(value)
    return time.date(dates.stamp(value)).weekday >= 6
end

function dates.is_weekday(value)
    return not dates.is_weekend(value)
end

function dates.same_day(a, b)
    return time.format("%Y-%m-%d", dates.stamp(a)) == time.format("%Y-%m-%d", dates.stamp(b))
end

local WEEKDAYS = { "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday" }

--- "Monday" ... "Sunday"
function dates.weekday_name(value)
    return WEEKDAYS[time.date(dates.stamp(value)).weekday]
end

--- The ISO week number (1-53), as used in most of Europe.
function dates.week_number(value)
    local stamp = dates.start_of_day(value)
    local d = time.date(stamp)
    -- The Thursday of this week decides which year the week belongs to.
    local thursday = time.date(dates.add_days(stamp, 4 - d.weekday))
    return (thursday.yday - 1) // 7 + 1
end

--- Next time the clock shows `hh:mm` ("07:30"), today or tomorrow.
function dates.next_time(clock, from)
    local hour, min = tostring(clock):match("^(%d%d?):(%d%d)$")
    if not hour then
        error("dates.next_time: use a time like \"07:30\"", 2)
    end
    from = dates.stamp(from)
    local d = time.date(from)
    local at = time.make({ year = d.year, month = d.month, day = d.day, hour = tonumber(hour), min = tonumber(min) })
    if at <= from then
        at = dates.add_days(at, 1)
    end
    return at
end

--- Seconds as readable text: 3725 --> "1 h 2 min 5 s"
function dates.duration(seconds)
    seconds = math.floor(math.abs(seconds) + 0.5)
    if seconds == 0 then
        return "0 s"
    end
    local parts = {}
    local units = { { DAY, "d" }, { 3600, "h" }, { 60, "min" }, { 1, "s" } }
    for _, unit in ipairs(units) do
        local n = seconds // unit[1]
        if n > 0 then
            parts[#parts + 1] = n .. " " .. unit[2]
            seconds = seconds - n * unit[1]
        end
    end
    return table.concat(parts, " ")
end

--- How long ago, in words: "just now", "5 minutes ago", "3 days ago", "in 2 hours".
function dates.ago(value, now)
    local diff = dates.stamp(now) - dates.stamp(value)
    local future = diff < 0
    diff = math.abs(diff)
    local text
    if diff < 45 then
        return "just now"
    elseif diff < 90 then
        text = "1 minute"
    elseif diff < 3600 then
        text = math.floor(diff / 60 + 0.5) .. " minutes"
    elseif diff < 5400 then
        text = "1 hour"
    elseif diff < DAY then
        text = math.floor(diff / 3600 + 0.5) .. " hours"
    elseif diff < 2 * DAY then
        text = "1 day"
    elseif diff < 30 * DAY then
        text = math.floor(diff / DAY + 0.5) .. " days"
    elseif diff < 365 * DAY then
        local months = math.floor(diff / (30 * DAY) + 0.5)
        text = months == 1 and "1 month" or (months .. " months")
    else
        local years = math.floor(diff / (365 * DAY) + 0.5)
        text = years == 1 and "1 year" or (years .. " years")
    end
    return future and ("in " .. text) or (text .. " ago")
end

--- A timestamp as "2026-09-29".
function dates.iso(value)
    return time.format("%Y-%m-%d", dates.stamp(value))
end

--- A timestamp as "2026-09-29 14:05".
function dates.iso_time(value)
    return time.format("%Y-%m-%d %H:%M", dates.stamp(value))
end

return dates
