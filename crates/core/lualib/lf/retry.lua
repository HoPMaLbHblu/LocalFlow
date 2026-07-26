-- lf.retry: trying again, timing, and "not too often".
--
--   local retry = require("lf.retry")
--   local page = retry.times(3, function() return http.get(url) end, { wait = 2 })
--   local seconds, result = retry.measure(function() return fs.largest("~", 5) end)
--   if retry.at_most_every("backup-warning", time.hours(6)) then notify("...") end

local retry = {}

--- Run `fn` up to `times` times until it doesn't fail. Waits `options.wait`
--- seconds between tries (default 1), doubling each time if `options.backoff` is true.
--- Returns what `fn` returns; if every try fails, raises the last error.
function retry.times(times, fn, options)
    options = options or {}
    local pause = options.wait or 1
    local last_error
    for attempt = 1, times do
        local ok, a, b, c = pcall(fn, attempt)
        if ok then
            return a, b, c
        end
        last_error = a
        if options.on_error then
            options.on_error(a, attempt)
        end
        if attempt < times and pause > 0 then
            wait(pause)
            if options.backoff then
                pause = pause * 2
            end
        end
    end
    error("failed " .. times .. " times; last error: " .. tostring(last_error), 2)
end

--- Like times(), but "failure" also means `fn` returned nil or false.
function retry.until_ok(times, fn, options)
    return retry.times(times, function(attempt)
        local result = fn(attempt)
        if not result then
            error("no result", 0)
        end
        return result
    end, options)
end

--- Check `condition()` every `every` seconds (default 1) until it is true or
--- `seconds` have passed. Returns true if it became true in time.
function retry.wait_for(condition, seconds, every)
    every = every or 1
    local waited = 0
    while true do
        if condition() then
            return true
        end
        if waited >= seconds then
            return false
        end
        wait(every)
        waited = waited + every
    end
end

--- How long `fn` takes, in seconds (whole seconds), and what it returned.
function retry.measure(fn, ...)
    local started = time.now()
    local result = { fn(...) }
    return time.now() - started, table.unpack(result)
end

--- True at most once every `seconds` for `key` (remembered between runs with store).
--- Handy to avoid sending the same warning every minute.
function retry.at_most_every(key, seconds)
    local store_key = "lf.retry:" .. key
    local last = store.get(store_key, 0)
    local now = time.now()
    if now - last < seconds then
        return false
    end
    store.set(store_key, now)
    return true
end

--- Forget at_most_every's memory for `key`.
function retry.reset(key)
    store.delete("lf.retry:" .. key)
end

--- Run `fn` safely: returns true and the result, or false and a readable error,
--- without stopping the script.
function retry.try(fn, ...)
    local ok, result = pcall(fn, ...)
    if ok then
        return true, result
    end
    local message = tostring(result):gsub("^automation:%d+: ", "")
    return false, message
end

return retry
