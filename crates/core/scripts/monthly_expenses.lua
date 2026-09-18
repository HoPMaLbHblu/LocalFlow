-- Adds up a simple spending list by category for this month and last month.
-- Keep your spending in Documents/expenses.csv (Excel can save CSV files):
--
--   date,category,amount,note
--   2026-09-03,Food,23.50,Groceries
--   2026-09-04,Transport,2.80,Bus
--
-- The first run creates an example file for you.

local tables = require("lf.tables")
local dates = require("lf.dates")
local report = require("lf.report")
local strings = require("lf.strings")

local EXAMPLE = "date,category,amount,note\n"
    .. time.today() .. ",Food,23.50,Example: groceries\n"
    .. time.today() .. ",Transport,2.80,Example: bus ticket\n"

local function month_key(stamp)
    return time.format("%Y-%m", stamp)
end

automation {
    name = "Monthly spending summary",

    run = function(ctx)
        local file = "~/Documents/expenses.csv"
        if not fs.exists(file) then
            fs.write(file, EXAMPLE)
            notify("Created " .. file .. " with an example. Add your spending there.")
        end

        local this_month = month_key(time.now())
        local last_month = month_key(dates.add_months(time.now(), -1))
        local totals = { [this_month] = {}, [last_month] = {} }

        for line, row in ipairs(csv.read(file)) do
            local stamp = row.date and time.parse(row.date)
            -- "23,50" and "23.50" both work.
            local cleaned = (row.amount or ""):gsub(",", ".")
            local amount = tonumber(cleaned)
            if not stamp or not amount then
                log("Skipped line " .. (line + 1) .. ": needs a date like 2026-09-03 and an amount")
            elseif totals[month_key(stamp)] then
                local month = totals[month_key(stamp)]
                local category = strings.title(strings.trim(row.category or "Other"))
                month[category] = (month[category] or 0) + amount
            end
        end

        local r = report.new("Spending " .. this_month)
        local categories = tables.unique(tables.concat(tables.keys(totals[this_month]), tables.keys(totals[last_month])))
        local rows = {}
        for _, c in ipairs(categories) do
            local now, before = totals[this_month][c] or 0, totals[last_month][c] or 0
            rows[#rows + 1] = { c, strings.number(now, 2), strings.number(before, 2) }
        end
        r:table({ "Category", this_month, last_month }, tables.sort_by(rows, function(row) return row[1] end))
        local sum = tables.sum(tables.values(totals[this_month]))
        r:blank():line("Total this month: " .. strings.number(sum, 2))
        r:log()
        notify("Spent " .. strings.number(sum, 2) .. " so far this month")
    end
}
