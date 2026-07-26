-- lf.template: fill {placeholders} in text.
--
--   local template = require("lf.template")
--   template.render("Hi {name}, you have {count} new files", { name = "Sam", count = 3 })
--   template.render("{title|upper} ({size|size})", { title = "Report", size = 2048 })
--
-- {name}          a value (nested values with dots: {user.name})
-- {name|filter}   a value changed by a filter (see FILTERS below); filters can be chained
-- {name?text}     `text` when the value is missing or empty
-- {{ and }}       a literal { and }

local template = {}

local FILTERS = {
    upper = string.upper,
    lower = string.lower,
    trim = function(s)
        return (s:gsub("^%s+", ""):gsub("%s+$", ""))
    end,
    title = function(s)
        return (s:gsub("(%a)([%w']*)", function(a, b)
            return a:upper() .. b:lower()
        end))
    end,
    size = function(s)
        local bytes = tonumber(s) or 0
        local units = { "bytes", "KB", "MB", "GB", "TB" }
        local unit = 1
        while bytes >= 1024 and unit < #units do
            bytes, unit = bytes / 1024, unit + 1
        end
        if unit == 1 then
            return string.format("%d bytes", bytes)
        end
        return (string.format("%.1f", bytes):gsub("%.0$", "")) .. " " .. units[unit]
    end,
    date = function(s)
        return time.format("%Y-%m-%d", tonumber(s))
    end,
    datetime = function(s)
        return time.format("%Y-%m-%d %H:%M", tonumber(s))
    end,
    round = function(s)
        local n = tonumber(s)
        return n and tostring(math.floor(n + 0.5)) or s
    end,
    percent = function(s)
        local n = tonumber(s)
        return n and (string.format("%.0f", n) .. "%") or s
    end,
    count = function(s)
        return s
    end,
}

--- Add your own filter: template.filter("shout", function(s) return s .. "!" end)
function template.filter(name, fn)
    FILTERS[name] = fn
end

local function lookup(values, path)
    local value = values
    for part in path:gmatch("[^%.]+") do
        if type(value) ~= "table" then
            return nil
        end
        local index = tonumber(part)
        value = value[index or part]
        if value == nil and index then
            value = nil
        end
    end
    return value
end

local function expand(expression, values)
    local body, fallback = expression:match("^(.-)%?(.*)$")
    body = body or expression
    local parts = {}
    for part in body:gmatch("[^|]+") do
        parts[#parts + 1] = part:match("^%s*(.-)%s*$")
    end
    local value = lookup(values, parts[1] or "")
    if type(value) == "table" then
        value = #value
    end
    if value == nil or value == "" then
        if fallback then
            return fallback
        end
        return ""
    end
    local text = tostring(value)
    for i = 2, #parts do
        local filter = FILTERS[parts[i]]
        if not filter then
            error("template: unknown filter \"" .. parts[i] .. "\"", 4)
        end
        text = tostring(filter(text))
    end
    return text
end

--- Fill the placeholders in `text` with `values`.
function template.render(text, values)
    values = values or {}
    local OPEN, CLOSE = "\1", "\2"
    local out = text:gsub("{{", OPEN):gsub("}}", CLOSE)
    out = out:gsub("{([^{}]*)}", function(expression)
        return expand(expression, values)
    end)
    return (out:gsub(OPEN, "{"):gsub(CLOSE, "}"))
end

--- The placeholder names used in `text`, e.g. { "name", "count" }.
function template.fields(text)
    local names, seen = {}, {}
    for expression in text:gsub("{{", ""):gsub("}}", ""):gmatch("{([^{}]*)}") do
        local name = expression:match("^%s*([^|%?]-)%s*[|%?]") or expression:match("^%s*(.-)%s*$")
        if not seen[name] then
            seen[name] = true
            names[#names + 1] = name
        end
    end
    return names
end

--- Render a template file (read with fs.read) and optionally save the result.
function template.render_file(source, values, destination)
    local text = template.render(fs.read(source), values)
    if destination then
        fs.write(destination, text)
    end
    return text
end

return template
