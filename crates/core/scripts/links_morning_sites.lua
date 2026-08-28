-- Opens a few sites every morning at 8:00: news, weather, your calendar.
-- The "Morning" link set is created with examples the first time; change it on the Links page.

automation {
    name = "Morning sites",

    run = function(ctx)
        if not links.get("Morning") then
            links.save("Morning", "https://news.ycombinator.com\nhttps://www.bbc.com/news\nhttps://calendar.google.com")
        end
        links.open("Morning")
    end
}
