-- Locks files with a password. Put files in Documents/Private/To lock; each one
-- becomes a ".locked" file that can only be opened with your password, and the
-- original goes to the Recycle Bin (empty it yourself when you're sure).
--
-- To open a file again, use the snippet in the guide or:
--   crypto.decrypt("~/Documents/Private/report.pdf.locked", "~/Desktop/report.pdf", "your password")
--
-- IMPORTANT: if you forget the password, the files can't be opened by anyone,
-- including you. Write it down somewhere safe.

local paths = require("lf.paths")

automation {
    name = "Lock private files",

    run = function(ctx)
        local password = "change me"       -- choose your own (at least 8 characters)
        if password == "change me" then
            error("Open this automation and choose your own password first (line 17)")
        end

        local inbox = paths.ensure_dir("~/Documents/Private/To lock")
        local safe = "~/Documents/Private"
        local locked = 0

        for _, file in ipairs(fs.list(inbox, "*")) do
            local target = paths.unique(paths.join(safe, paths.name(file) .. ".locked"))
            crypto.encrypt(file, target, password)
            -- Make sure it opens again before throwing the original away.
            local check = paths.join(inbox, ".check-" .. paths.name(file))
            crypto.decrypt(target, check, password)
            if fs.hash(check) == fs.hash(file) then
                fs.delete(check)
                fs.delete(file)
                locked = locked + 1
            else
                error("Checking " .. paths.name(file) .. " failed, the original was kept")
            end
        end

        if locked > 0 then
            notify("Locked " .. locked .. " file(s) in " .. safe)
        end
    end
}
