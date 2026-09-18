-- Scanners and phones give files names like "scan0042.pdf". This puts the
-- date in front as soon as a file arrives, so they sort nicely:
-- Documents/Scans/scan0042.pdf  ->  Documents/Scans/2026-09-29 scan0042.pdf

local paths = require("lf.paths")

automation {
    name = "Put the date in scan names",

    run = function(ctx)
        -- Started by the folder watch: ctx.file is the new scan.
        local files = ctx.file and { ctx.file } or fs.list("~/Documents/Scans", "*.pdf")

        for _, file in ipairs(files) do
            local name = paths.name(file)
            -- Already starts with a date? Leave it alone.
            if not name:match("^%d%d%d%d%-%d%d%-%d%d") then
                local dated = time.format("%Y-%m-%d", fs.modified(file)) .. " " .. name
                local target = paths.unique(paths.with_name(file, dated))
                fs.rename(file, paths.name(target))
                log(name .. " -> " .. paths.name(target))
            end
        end
    end
}
