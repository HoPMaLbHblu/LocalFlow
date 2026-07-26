-- Keeps your backup folder from filling the disk: only the newest backups are
-- kept, older ones go to the Recycle Bin (so you can still get them back).
-- Works well together with the "Daily zip backup" template.

local paths = require("lf.paths")
local tables = require("lf.tables")

automation {
    name = "Keep only recent backups",

    run = function(ctx)
        local folder = "~/Backups"
        local pattern = "*.zip"
        local keep = 7

        if not fs.exists(folder) then
            log("No backup folder yet: " .. folder)
            return
        end

        local newest_first = tables.sort_by(fs.list(folder, pattern), fs.modified, true)
        local old = tables.slice(newest_first, keep + 1)
        local freed = 0
        for _, file in ipairs(old) do
            freed = freed + fs.size(file)
            fs.delete(file)
            log("Moved to Recycle Bin: " .. paths.name(file))
        end

        log("Kept " .. math.min(#newest_first, keep) .. " backups")
        if #old > 0 then
            notify("Removed " .. #old .. " old backups (" .. paths.size_text(freed) .. ")")
        end
    end
}
