-- Keeps a history of the text you copy: one file per day in
-- Documents/Clipboard history. It remembers the last text it saved,
-- so the same text is only written once.

automation {
    name = "Clipboard history",

    run = function(ctx)
        local text = clipboard.get()
        if not text or text == "" then
            return
        end
        if text == store.get("last") then
            return -- nothing new since last time
        end
        store.set("last", text)

        local file = fs.join("~/Documents/Clipboard history", time.today() .. ".txt")
        fs.append(file, time.format("[%H:%M:%S] ") .. text .. "\n\n")
        log("Saved " .. #text .. " characters")
    end
}
