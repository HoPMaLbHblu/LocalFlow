-- Once a week, lists the apps that have updates (from winget, which is
-- built into Windows) and saves the list to Documents/LocalFlow reports.
-- It only looks; the "Update all my apps" template installs them.

automation {
    name = "Check for app updates",

    run = function(ctx)
        local updates = packages.updates()
        if #updates == 0 then
            log("All apps are up to date")
            return
        end

        local lines = { "App updates, " .. time.format("%Y-%m-%d"), "" }
        for _, u in ipairs(updates) do
            lines[#lines + 1] = string.format("%s: %s -> %s", u.name, u.version, u.available)
        end
        local report = "~/Documents/LocalFlow reports/app updates " .. time.format("%Y-%m-%d") .. ".txt"
        fs.write(report, table.concat(lines, "\n"))
        log(table.concat(lines, "\n"))
        notify(#updates .. " app(s) can be updated. The list is in Documents/LocalFlow reports.")
    end
}
