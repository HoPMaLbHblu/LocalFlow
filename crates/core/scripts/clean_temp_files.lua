-- Leftovers from interrupted downloads and programs (".tmp", ".crdownload",
-- ".part", ...) pile up in Downloads. This moves the ones older than a week
-- to the Recycle Bin every Sunday. Newer ones are left alone in case a
-- download is still running.

local paths = require("lf.paths")
local report = require("lf.report")

automation {
    name = "Clean up leftover temp files",

    run = function(ctx)
        local folder = "~/Downloads"
        local kinds = { "tmp", "temp", "crdownload", "part", "partial", "download", "bak", "old" }
        local older_than = time.days(7)

        local r = report.new("Temp files")
        for _, file in ipairs(fs.find(folder, "*")) do
            if paths.has_ext(file, table.unpack(kinds)) and time.now() - fs.modified(file) > older_than then
                r:count("files")
                r:count("bytes", fs.size(file))
                fs.delete(file)
                log("Recycled " .. paths.name(file))
            end
        end

        local count = r.counts.files or 0
        if count > 0 then
            notify("Moved " .. count .. " leftover files (" .. paths.size_text(r.counts.bytes) .. ") to the Recycle Bin")
        else
            log("Nothing to clean")
        end
    end
}
