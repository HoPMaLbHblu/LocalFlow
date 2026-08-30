-- Press Ctrl+Alt+H during a Dota 2 match to see what the live match helper sees.
--
-- The live match helper itself runs inside LocalFlow, not in this script. Switch it on in
-- Settings › Dota 2 companion › "Live match helper". During a match it then tells you:
--   * "You can now afford <item> (next in your plan)", once per item of your item plan;
--   * about 15 seconds before runes, Shrines of Wisdom, Lotus Pools, the Tormentor,
--     new neutral item tiers, and day and night.
-- At most one notification every 20 seconds, and never the same one twice in a match.
--
-- It needs Game State Integration (Valve's official feature: the game tells LocalFlow its
-- clock, your gold and your items; nothing reads the game's memory). Install it in the same
-- settings page. If you installed it with an older LocalFlow, LocalFlow updates the file by
-- itself: restart Dota 2 once afterwards.
--
-- Change the hotkey in the editor (Triggers) if Ctrl+Alt+H is taken.

local function ago(seconds)
    if not seconds then return "never" end
    local d = time.now() - seconds
    if d < 0 then d = 0 end
    return d .. " s ago"
end

automation {
    name = "Dota 2: live match helper",

    run = function(ctx)
        local ok, s = pcall(dota.status)
        if not ok then
            notify("Dota 2 live helper: " .. tostring(s))
            return
        end

        if not s.gsi_installed then
            notify("Dota 2 live helper: install Game State Integration first (Settings › Dota 2 companion), " ..
                "then switch on \"Live match helper\" there and restart Dota 2.")
            return
        end
        if not s.listening then
            notify("Dota 2 live helper: LocalFlow isn't receiving game data" ..
                (s.listen_error and (" (" .. s.listen_error .. ")") or "") .. ".")
            return
        end

        log("Game State Integration: port " .. tostring(s.port) .. ", last update " .. ago(s.last_update))
        log("State: " .. tostring(s.state) .. (s.hero_name and (", hero " .. s.hero_name) or ""))

        if s.state == "playing" then
            notify("Dota 2 live helper: match in progress" ..
                (s.hero_name and (" as " .. s.hero_name:gsub("^npc_dota_hero_", "")) or "") ..
                ", game data " .. ago(s.last_update) .. ". Item and timing notifications come by themselves " ..
                "when \"Live match helper\" is on in Settings › Dota 2 companion.")
        else
            notify("Dota 2 live helper: no match in progress (state: " .. tostring(s.state) .. "). " ..
                "Switch on \"Live match helper\" in Settings › Dota 2 companion; it starts at the horn.")
        end
    end
}
