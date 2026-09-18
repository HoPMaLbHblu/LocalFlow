-- Every morning, creates a fresh journal page for today, ready to write in:
-- Documents/Journal/2026/09/2026-09-29 Tuesday.md

local paths = require("lf.paths")
local dates = require("lf.dates")
local template = require("lf.template")

local PAGE = [[
# {weekday}, {date}

Week {week} · {days_left} days left this year

## Plans for today
-

## Notes


## Good things today
-
]]

automation {
    name = "Daily journal page",

    run = function(ctx)
        local now = time.now()
        local d = time.date(now)
        local folder = paths.join("~/Documents/Journal", tostring(d.year), string.format("%02d", d.month))
        local file = paths.join(folder, dates.iso(now) .. " " .. dates.weekday_name(now) .. ".md")

        if fs.exists(file) then
            log("Today's page already exists: " .. file)
            return
        end

        local new_year = dates.make(d.year + 1, 1, 1)
        fs.write(file, template.render(PAGE, {
            weekday = dates.weekday_name(now),
            date = dates.iso(now),
            week = dates.week_number(now),
            days_left = dates.days_between(now, new_year) - 1,
        }))
        notify("Journal page ready: " .. paths.name(file))
    end
}
