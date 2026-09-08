-- Lists the biggest files, so you can decide what to delete.
--
-- To scan a whole disk: add it (for example C:\) in Settings > Allowed folders,
-- raise Settings > Script time limit to a few minutes, and set folder = "C:/".

automation {
    name = "Find the largest files",

    run = function(ctx)
        local folder = "~"
        local files, complete = fs.largest(folder, 20)

        for i, file in ipairs(files) do
            local mb = file.size / (1024 * 1024)
            log(string.format("%2d. %9.1f MB   %s", i, mb, file.path))
        end

        if not complete then
            log("Stopped at the time limit, so the list may be incomplete.")
        end
    end
}
