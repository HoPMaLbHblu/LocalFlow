-- A short review of your last Dota 2 match, shown when the game closes.
--
-- Set it up once in Settings › Dota 2 companion › Post-game review: paste your Dotabuff,
-- OpenDota or STRATZ profile link (or your Steam id). The id is stored on this PC; only public
-- match data is requested from OpenDota. OpenDota can only see your matches when
-- "Expose Public Match Data" is on in Dota 2 (Settings › Options › Social).
--
-- OpenDota needs a moment after a game, so this waits about a minute first (never past the
-- script's time limit: raise it in Settings › Scripts if the review often comes too early).
-- Then the Dota 2 window opens; its Review tab has the details: benchmarks, notes, recent matches.

local function reason(e)
    local text = tostring(e)
    return text:match("dota%.[%w_]+: ([^\n]*)") or text
end

automation {
    name = "Dota 2: post-game review",

    run = function(ctx)
        local ok, review = pcall(dota.last_match, 60)
        if not ok then
            notify("Dota 2 review: " .. reason(review))
            return
        end

        local s = review.summary
        -- An old match means OpenDota doesn't have the game you just played yet.
        local old = time.now() - (s.start_time + s.duration_secs) > 2 * 60 * 60

        local text = string.format("%s: %s, %d/%d/%d, GPM %d, XPM %d",
            s.hero, s.won and "won" or "lost", s.kills, s.deaths, s.assists, s.gpm, s.xpm)
        if old then
            text = "OpenDota doesn't list the game you just played yet. Your last match there: " .. text
        end
        for _, b in ipairs(review.benchmarks) do
            if b.percentile then
                log(string.format("%s: %s (better than %d%%)", b.metric, b.value, math.floor(b.percentile * 100 + 0.5)))
            end
        end
        for _, note in ipairs(review.notes) do
            log(note.text)
        end
        if review.notes[1] then
            text = text .. "\n" .. review.notes[1].text
        end
        notify(text)
        dota.show()
    end
}
