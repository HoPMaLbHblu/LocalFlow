-- A new wallpaper every day, picked at random from Pictures/Wallpapers.
-- Put your favourite pictures in that folder.
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Wallpaper of the day",

    run = function(ctx)
        local folder = "~/Pictures/Wallpapers"
        local pictures = {}
        for _, pattern in ipairs({ "*.jpg", "*.jpeg", "*.png", "*.bmp" }) do
            for _, file in ipairs(fs.list(folder, pattern)) do
                pictures[#pictures + 1] = file
            end
        end
        if #pictures == 0 then
            notify("Put some pictures in " .. folder .. " first")
            return
        end

        -- Don't pick the same picture twice in a row.
        local last = store.get("last", "")
        local choice = pictures[math.random(#pictures)]
        if choice == last and #pictures > 1 then
            choice = pictures[(math.random(#pictures - 1) % #pictures) + 1]
        end
        desktop.set_wallpaper(choice, "fill")
        store.set("last", choice)
        log("Wallpaper: " .. fs.basename(choice))
    end
}
