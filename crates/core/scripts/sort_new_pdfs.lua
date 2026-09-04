-- Runs by itself whenever a PDF lands in Downloads (see "Watch folder" above),
-- and files it into Documents/PDF/<year>-<month>.

automation {
    name = "File new PDFs",

    run = function(ctx)
        if not ctx.file then
            log("This automation runs when a new PDF appears in Downloads.")
            log("Save it, then download a PDF to see it work.")
            return
        end

        local folder = fs.join("~/Documents/PDF", time.format("%Y-%m"))
        local target = fs.join(folder, fs.basename(ctx.file))

        if fs.exists(target) then
            log("Already filed: " .. fs.basename(ctx.file))
        else
            fs.move(ctx.file, target)
            notify("Filed " .. fs.basename(ctx.file))
        end
    end
}
