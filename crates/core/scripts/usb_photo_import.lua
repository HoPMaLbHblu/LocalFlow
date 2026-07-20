-- When a USB drive or memory card is plugged in, copies its photos and videos
-- into Pictures/Imported/<today>. Files already imported are skipped.
-- The drive is in ctx.drive; for this run LocalFlow may read it.

automation {
    name = "Import photos from a memory card",

    run = function(ctx)
        if not ctx.drive then
            log("This runs when a USB drive or memory card is plugged in.")
            return
        end

        local target = fs.join("~/Pictures/Imported", time.today())
        local copied = 0

        for _, pattern in ipairs({ "*.jpg", "*.jpeg", "*.png", "*.heic", "*.mp4", "*.mov" }) do
            for _, file in ipairs(fs.find(ctx.drive, pattern)) do
                local destination = fs.join(target, fs.basename(file))
                if not fs.exists(destination) then
                    fs.copy(file, destination)
                    copied = copied + 1
                end
            end
        end

        if copied > 0 then
            notify("Imported " .. copied .. " photo(s) and video(s) from " .. ctx.drive)
        else
            log("No new photos on " .. ctx.drive)
        end
    end
}
