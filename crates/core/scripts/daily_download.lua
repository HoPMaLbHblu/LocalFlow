-- Downloads a file every morning into Downloads/Daily, with the date in its name:
-- a report, a price list, a backup from your site. Change the address below.

automation {
    name = "Daily download",

    run = function(ctx)
        local url = "https://example.com/"
        local name = time.format("%Y-%m-%d") .. " example.html"

        local file = http.download(url, "~/Downloads/Daily/" .. name)
        log(string.format("Saved %s (%d KB)", fs.basename(file), math.ceil(fs.size(file) / 1024)))
    end
}
