-- Makes small copies of photos so they're quick to send by e-mail or chat.
-- Put photos in Pictures/To share; smaller copies appear in Pictures/To share/small.
-- The originals are not changed.

local paths = require("lf.paths")

automation {
    name = "Shrink photos for sharing",

    run = function(ctx)
        local folder = "~/Pictures/To share"
        local max_size = 1600              -- longest side, in pixels

        paths.ensure_dir(folder)
        local out_folder = paths.join(folder, "small")
        local made, saved = 0, 0

        for _, photo in ipairs(fs.list(folder, "*")) do
            if paths.has_ext(photo, "jpg", "jpeg", "png", "webp") then
                local target = paths.join(out_folder, paths.stem(photo) .. ".jpg")
                if not fs.exists(target) then
                    image.resize(photo, target, max_size)
                    saved = saved + fs.size(photo) - fs.size(target)
                    made = made + 1
                    log("Shrunk " .. paths.name(photo))
                end
            end
        end

        notify("Made " .. made .. " small copies, saving " .. paths.size_text(math.max(saved, 0)))
    end
}
