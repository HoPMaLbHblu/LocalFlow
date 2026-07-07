-- The smallest useful automation.
-- Click "Run now" and check the output below.

automation {
    name = "Hello world",

    run = function(ctx)
        log("Hello from " .. ctx.name .. "!")
        log("This run was started by: " .. ctx.trigger)

        if fs.exists("~/Downloads") then
            local files = fs.list("~/Downloads", "*")
            log("Your Downloads folder has " .. #files .. " files.")
        end

        notify("Hello world finished")
    end
}
