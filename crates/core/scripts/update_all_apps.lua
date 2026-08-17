-- Updates all your apps with winget, after asking you first.
-- Some apps may need to close while they update.
--
-- Needs "Allow system control" (under "More triggers and permissions").

automation {
    name = "Update all my apps",

    run = function(ctx)
        local updates = packages.updates()
        if #updates == 0 then
            notify("All apps are up to date")
            return
        end

        local names = {}
        for _, u in ipairs(updates) do names[#names + 1] = u.name end
        if not ask("Update these apps?\n\n" .. table.concat(names, "\n"), "Update apps") then
            log("Cancelled")
            return
        end

        packages.upgrade("all")
        notify("Updated " .. #updates .. " app(s)")
    end
}
