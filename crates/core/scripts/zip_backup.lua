-- Zips a folder into Backups, with today's date in the file name.

automation {
    name = "Daily zip backup",

    run = function(ctx)
        local source = "~/Documents/Notes"
        if not fs.exists(source) then
            log(source .. " does not exist, so there is nothing to back up.")
            return
        end

        local target = fs.join("~/Backups", "Notes " .. time.today() .. ".zip")
        local count = zip.create(target, source)
        log("Backed up " .. count .. " files to " .. target)
    end
}
