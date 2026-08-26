-- Press Ctrl+Alt+B for an item plan for your hero against this enemy draft.
-- Your hero comes from Game State Integration (Settings › Dota 2 companion) or from the
-- Dota 2 window, where you can also pick it yourself. The window opens with the full plan:
-- starting items, core items, situational items and why.
--
-- Change the hotkey in the editor (Triggers) if Ctrl+Alt+B is taken.
-- For another hero, write its name: dota.build("Axe")

local function reason(e)
    local text = tostring(e)
    return text:match("dota%.[%w_]+: ([^\n]*)") or text
end

automation {
    name = "Dota 2: item build",

    run = function(ctx)
        local ok, plan = pcall(dota.build)
        if not ok then
            notify("Dota 2 build: " .. reason(plan))
            dota.show()
            return
        end

        local core = {}
        for _, item in ipairs(plan.core) do
            core[#core + 1] = item.item
        end
        local starting = {}
        for _, item in ipairs(plan.starting) do
            starting[#starting + 1] = item.item
        end
        log("Starting items: " .. table.concat(starting, ", "))
        for _, change in ipairs(plan.adaptations) do
            log("Against this draft: " .. change.text)
        end

        notify(plan.hero .. ": " .. table.concat(core, " → "))
        dota.show()
    end
}
