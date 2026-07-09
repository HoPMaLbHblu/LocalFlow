-- Move screenshots off the Desktop into Pictures/Screenshots.

automation {
    name = "Tidy screenshots",

    run = function(ctx)
        local destination = "~/Pictures/Screenshots"
        local patterns = { "Screenshot*.png", "Screen Shot*.png" }
        local moved = 0

        for _, pattern in ipairs(patterns) do
            for _, file in ipairs(fs.list("~/Desktop", pattern)) do
                local target = fs.join(destination, fs.basename(file))
                if not fs.exists(target) then
                    fs.move(file, target)
                    moved = moved + 1
                    log("Moved " .. fs.basename(file))
                end
            end
        end

        log("Moved " .. moved .. " screenshot(s)")
    end
}
