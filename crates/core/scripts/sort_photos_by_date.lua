-- Sorts photos into year and month folders by the date the camera saved in
-- them (when the photo was *taken*, not when it was copied).
-- Pictures/Unsorted/IMG_1234.jpg  ->  Pictures/Photos/2026/03 March/IMG_1234.jpg

local paths = require("lf.paths")

local MONTHS = { "January", "February", "March", "April", "May", "June",
                 "July", "August", "September", "October", "November", "December" }

automation {
    name = "Sort photos by date taken",

    run = function(ctx)
        local inbox = "~/Pictures/Unsorted"
        local library = "~/Pictures/Photos"

        paths.ensure_dir(inbox)
        local moved, guessed = 0, 0

        for _, photo in ipairs(fs.list(inbox, "*")) do
            if paths.has_ext(photo, "jpg", "jpeg", "heic", "png", "tif", "tiff", "dng") then
                local taken = image.taken(photo)
                if not taken then
                    -- No camera date (e.g. screenshots): use the file's date instead.
                    taken = fs.modified(photo)
                    guessed = guessed + 1
                end
                local d = time.date(taken)
                local folder = paths.join(library, tostring(d.year), string.format("%02d %s", d.month, MONTHS[d.month]))
                local target = paths.unique(paths.join(folder, paths.name(photo)))
                fs.move(photo, target)
                moved = moved + 1
            end
        end

        if moved > 0 then
            notify("Sorted " .. moved .. " photos (" .. guessed .. " without a camera date)")
        else
            log("Nothing to sort in " .. inbox)
        end
    end
}
