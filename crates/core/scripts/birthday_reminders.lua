-- Reminds you of birthdays today and in the next few days.
-- Keep them in Documents/birthdays.csv (the first run creates an example):
--
--   name,birthday
--   Grandma,1950-03-14
--   Alex,07-21          <- the year is optional

local dates = require("lf.dates")
local strings = require("lf.strings")

automation {
    name = "Birthday reminders",

    run = function(ctx)
        local file = "~/Documents/birthdays.csv"
        local days_ahead = 3

        if not fs.exists(file) then
            local d = time.date(time.now())
            fs.write(file, "name,birthday\nExample person," .. string.format("%02d-%02d", d.month, d.day) .. "\n")
            notify("Created " .. file .. ". Add your family and friends there.")
        end

        local today = dates.start_of_day()
        local year = time.date(today).year
        local found = 0

        for _, row in ipairs(csv.read(file)) do
            local born_year, month, day = strings.trim(row.birthday or ""):match("^(%d%d%d%d)%-(%d%d?)%-(%d%d?)$")
            if not month then
                month, day = strings.trim(row.birthday or ""):match("^(%d%d?)%-(%d%d?)$")
            end
            if month then
                local next_birthday = dates.make(year, tonumber(month), tonumber(day))
                if next_birthday < today then
                    next_birthday = dates.make(year + 1, tonumber(month), tonumber(day))
                end
                local days = dates.days_between(today, next_birthday)
                if days <= days_ahead then
                    local age = born_year and (" turns " .. (time.date(next_birthday).year - tonumber(born_year))) or ""
                    local when = days == 0 and "today" or (days == 1 and "tomorrow" or ("in " .. days .. " days"))
                    notify("🎂 " .. row.name .. age .. " " .. when .. " (" .. dates.weekday_name(next_birthday) .. ")")
                    found = found + 1
                end
            elseif row.name then
                log("Skipped " .. row.name .. ": write the birthday like 1990-07-21 or 07-21")
            end
        end
        log(found .. " birthday(s) in the next " .. days_ahead .. " days")
    end
}
