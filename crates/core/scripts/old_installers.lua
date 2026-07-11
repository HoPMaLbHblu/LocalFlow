-- Moves installers you downloaded more than 30 days ago into a separate folder.
-- Test run only lists them; Save and Run to actually move them.

automation {
    name = "Tidy old installers",

    run = function(ctx)
        local cutoff = time.now() - time.days(30)
        local target = "~/Downloads/Old installers"
        local found = 0

        for _, pattern in ipairs({ "*.exe", "*.msi" }) do
            for _, file in ipairs(fs.list("~/Downloads", pattern)) do
                if fs.modified(file) < cutoff then
                    found = found + 1
                    local age = math.floor((time.now() - fs.modified(file)) / time.days(1))
                    if ctx.trigger == "test" then
                        log("Would move " .. fs.basename(file) .. " (" .. age .. " days old)")
                    else
                        fs.move(file, fs.join(target, fs.basename(file)))
                        log("Moved " .. fs.basename(file) .. " (" .. age .. " days old)")
                    end
                end
            end
        end

        log(found .. " old installer(s)")
    end
}
