-- Many websites save pictures as .webp, which some programs can't open.
-- Every time a .webp lands in Downloads, this makes a .png copy next to it.

local paths = require("lf.paths")

automation {
    name = "Turn WebP downloads into PNG",

    run = function(ctx)
        -- Started by the folder watch: ctx.file is the new picture.
        -- Pressing Run instead converts every .webp in Downloads.
        local files = ctx.file and { ctx.file } or fs.list("~/Downloads", "*.webp")

        for _, file in ipairs(files) do
            local png = paths.with_ext(file, "png")
            if not fs.exists(png) then
                image.convert(file, png)
                log("Converted " .. paths.name(file))
            end
        end
    end
}
