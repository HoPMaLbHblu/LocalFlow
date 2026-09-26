-- During the hero draft, press Ctrl+Alt+D: LocalFlow takes a screenshot, recognises the
-- heroes in the top bar, and suggests heroes for you. The Dota 2 window opens with the
-- details, where you can fix any slot it got wrong.
--
-- Change the hotkey in the editor (Triggers) if Ctrl+Alt+D is taken.
-- Screenshots stay on this PC. Suggestions support your decision; they don't promise a win.

local function reason(e)
    local text = tostring(e)
    return text:match("dota%.[%w_]+: ([^\n]*)") or text
end

automation {
    name = "Dota 2: draft assistant",

    run = function(ctx)
        local ok, result = pcall(dota.capture_draft)
        if not ok then
            notify("Dota 2 draft: " .. reason(result))
            dota.show()
            return
        end

        local lines = { string.format("Recognised %d heroes.", result.recognized) }
        for _, warning in ipairs(result.warnings) do
            log("Note: " .. warning)
        end
        if #result.draft.uncertain > 0 then
            lines[#lines + 1] = "Please check: " .. table.concat(result.draft.uncertain, ", ")
        end

        local found, suggestions = pcall(dota.suggest, 3)
        if found and #suggestions > 0 then
            local names = {}
            for _, s in ipairs(suggestions) do
                names[#names + 1] = s.hero
            end
            lines[#lines + 1] = "Try: " .. table.concat(names, ", ")
        elseif not found then
            lines[#lines + 1] = "No suggestions: " .. reason(suggestions)
        end

        notify(table.concat(lines, "\n"))
        dota.show()
    end
}
