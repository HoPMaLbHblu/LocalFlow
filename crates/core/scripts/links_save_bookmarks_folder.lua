-- Turns a bookmarks folder into a link set, so an automation can open all of it.
-- In Chrome or Edge: right-click the tab bar > "Bookmark all tabs" (Ctrl+Shift+D) and save
-- them into a folder called "LocalFlow tabs". Then run this. Your bookmarks are only read.

automation {
    name = "Save a bookmarks folder as a link set",

    run = function(ctx)
        local folder = "LocalFlow tabs"   -- the bookmarks folder
        local browser = "chrome"          -- chrome, edge, brave, opera, yandex
        local set = "Saved tabs"          -- the link set to create or replace

        local urls = links.import_bookmarks(folder, browser, set)
        notify(string.format("Saved %d tab(s) from \"%s\" as the link set \"%s\"", #urls, folder, set))
    end
}
