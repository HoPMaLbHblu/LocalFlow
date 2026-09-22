-- Press Ctrl+Alt+S to save a screenshot of all your screens to
-- Pictures/Screenshots. The file's path is copied, ready to paste.

automation {
    name = "Quick screenshot",

    run = function(ctx)
        local shot = screen.capture("~/Pictures/Screenshots/" .. time.format("%Y-%m-%d %H-%M-%S") .. ".png")
        clipboard.set(shot)
        notify("Screenshot saved: " .. fs.basename(shot))
    end
}
