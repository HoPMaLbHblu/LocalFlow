-- Move every PDF from Downloads into Documents/PDF.
-- The destination folder is created automatically.

automation {
    name = "Organize PDF files",

    run = function(ctx)
        local files = fs.list("~/Downloads", "*.pdf")

        for _, file in ipairs(files) do
            local target = "~/Documents/PDF/" .. fs.basename(file)

            if fs.exists(target) then
                log("Skipped (already exists): " .. fs.basename(file))
            else
                fs.move(file, target)
                log("Moved file: " .. file)
            end
        end

        if #files > 0 then
            notify("Organized " .. #files .. " PDF file(s)")
        end
    end
}
