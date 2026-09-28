-- Opens all your work tabs at once, every weekday at 9:00, in a new browser window.
-- Edit the "Work" link set on the Links page (the sidebar), or change the list below:
-- it is only used to create the set the first time.

automation {
    name = "Open my work tabs",

    run = function(ctx)
        local set = "Work"
        if not links.get(set) then
            links.save(set, {
                "https://mail.google.com",
                "https://calendar.google.com",
                "https://github.com",
            }, { browser = "default", new_window = true })
            notify("Created the link set \"Work\". Add your own tabs on the Links page.")
        end
        local opened = links.open(set)
        log("Opened " .. opened .. " tab(s)")
    end
}
