-- Takes a screenshot every hour while you use the PC, into
-- Pictures/Screen diary/<date>, so you can see what you worked on.
-- Days older than a week go to the Recycle Bin.

automation {
    name = "Screen diary",

    run = function(ctx)
        local folder = "~/Pictures/Screen diary"
        local keep_days = 7

        if system.idle_seconds() > 10 * 60 then
            log("Nobody is using the PC; no screenshot")
            return
        end
        local shot = screen.capture(fs.join(folder, time.format("%Y-%m-%d"), time.format("%H-%M") .. ".jpg"))
        log("Saved " .. shot)

        local oldest = time.format("%Y-%m-%d", time.now() - time.days(keep_days))
        for _, day in ipairs(fs.list_dirs(folder)) do
            if fs.basename(day) < oldest then
                fs.delete(day)
                log("Moved to the Recycle Bin: " .. fs.basename(day))
            end
        end
    end
}
