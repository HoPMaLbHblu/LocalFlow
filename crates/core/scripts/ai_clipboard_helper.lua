-- Copy some text, press Ctrl+Alt+E, and GigaChat explains it in simple words.
-- The answer is shown and copied, so you can paste it.
-- Set up GigaChat in Settings first.

automation {
    name = "Explain what I copied",

    run = function(ctx)
        if not ai.available() then
            notify("Set up GigaChat in Settings to use this")
            return
        end
        local text = clipboard.get()
        if not text or text == "" then
            notify("Copy some text first")
            return
        end

        local answer = ai.ask("Explain this in simple words, in at most 5 sentences, in the language of the text:\n\n" .. text:sub(1, 4000))
        clipboard.set(answer)
        notify(answer)
        log(answer)
    end
}
