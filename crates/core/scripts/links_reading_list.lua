-- One article a day: every evening at 20:00, opens the next link from your "Reading list"
-- link set and moves it to "Read", so nothing is lost. Add links to "Reading list" on the Links page.

automation {
    name = "One article a day",

    run = function(ctx)
        local items = links.get("Reading list")
        if not items or #items == 0 then
            notify("Your reading list is empty. Add links to \"Reading list\" on the Links page.")
            return
        end
        local url = items[1]
        links.open({ url })
        links.add("Read", url)
        links.remove("Reading list", 1)
        log("Opened " .. url .. " (" .. (#items - 1) .. " left)")
    end
}
