-- Finds files with exactly the same contents (not just the same name) and
-- saves a report, biggest first. Nothing is deleted: read the report and
-- decide yourself which copies to remove.

local paths = require("lf.paths")
local report = require("lf.report")

automation {
    name = "Find duplicate files",

    run = function(ctx)
        local folder = "~/Downloads"       -- where to look (subfolders too)
        local pattern = "*"                -- e.g. "*.jpg" for photos only

        local groups, complete = fs.duplicates(folder, pattern)
        local r = report.new("Duplicate files in " .. folder)

        local wasted = 0
        for _, group in ipairs(groups) do
            -- Every copy after the first is space you could free.
            wasted = wasted + group.size * (#group.files - 1)
            r:heading(#group.files .. " copies, " .. paths.size_text(group.size) .. " each")
            for _, file in ipairs(group.files) do
                r:item(file)
            end
        end

        if #groups == 0 then
            r:line("No duplicates found.")
        end
        if not complete then
            r:line("(Stopped early: the folder is very big. Raise Settings › Script time limit to check everything.)")
        end

        local file = paths.join("~/Documents/LocalFlow reports", "duplicates " .. time.today() .. ".txt")
        r:save(file)
        notify(#groups .. " sets of duplicates, " .. paths.size_text(wasted) .. " could be freed. Report: " .. file)
    end
}
