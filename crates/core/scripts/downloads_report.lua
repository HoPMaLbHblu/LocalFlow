-- Every Monday, writes a short report about your Downloads folder: how much
-- space each kind of file uses, and the biggest and oldest files.
-- Also saves the full list as a spreadsheet (CSV) you can open in Excel.

local paths = require("lf.paths")
local tables = require("lf.tables")
local dates = require("lf.dates")
local report = require("lf.report")

automation {
    name = "Weekly Downloads report",

    run = function(ctx)
        local folder = "~/Downloads"
        local files = tables.map(fs.list(folder, "*"), function(path)
            return {
                path = path,
                name = paths.name(path),
                kind = paths.ext(path) ~= "" and paths.ext(path) or "(none)",
                size = fs.size(path),
                modified = fs.modified(path),
            }
        end)

        local r = report.new("Downloads, week " .. dates.week_number())
        r:line(#files .. " files, " .. paths.size_text(tables.sum(files, function(f) return f.size end)) .. " in total")

        r:heading("By type")
        local rows = {}
        for kind, group in pairs(tables.group_by(files, function(f) return f.kind end)) do
            rows[#rows + 1] = { kind, #group, tables.sum(group, function(f) return f.size end) }
        end
        rows = tables.sort_by(rows, function(row) return row[3] end, true)
        r:table({ "Type", "Files", "Size" }, tables.map(tables.take(rows, 10), function(row)
            return { row[1], row[2], paths.size_text(row[3]) }
        end))

        r:heading("Biggest files")
        for _, f in ipairs(tables.take(tables.sort_by(files, function(f) return f.size end, true), 5)) do
            r:item(f.name .. " (" .. paths.size_text(f.size) .. ")")
        end

        r:heading("Oldest files")
        for _, f in ipairs(tables.take(tables.sort_by(files, function(f) return f.modified end), 5)) do
            r:item(f.name .. " (" .. dates.ago(f.modified) .. ")")
        end

        local out = "~/Documents/LocalFlow reports"
        r:save(paths.join(out, "downloads " .. time.today() .. ".txt"))
        csv.write(paths.join(out, "downloads " .. time.today() .. ".csv"), tables.map(files, function(f)
            return { name = f.name, type = f.kind, bytes = f.size, modified = dates.iso_time(f.modified) }
        end), { header = { "name", "type", "bytes", "modified" } })
        notify("Downloads report saved in " .. out)
    end
}
