-- Copy Markdown notes into a backup folder.
-- Files that were already backed up are skipped.

automation {
    name = "Back up notes",

    run = function(ctx)
        local source = "~/Documents/Notes"
        local backup = "~/Backups/Notes"

        if not fs.exists(source) then
            log("Nothing to back up: " .. source .. " does not exist")
            return
        end

        fs.mkdir(backup)

        local copied = 0
        for _, file in ipairs(fs.list(source, "*.md")) do
            local target = fs.join(backup, fs.basename(file))
            if not fs.exists(target) then
                fs.copy(file, target)
                copied = copied + 1
            end
        end

        log("Backed up " .. copied .. " new note(s)")
    end
}
