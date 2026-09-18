-- lf.strings: everyday text helpers.
--
--   local strings = require("lf.strings")
--   strings.trim("  hi  ")                 --> "hi"
--   strings.split("a,b,,c", ",")           --> { "a", "b", "", "c" }
--   strings.title("hello world")           --> "Hello World"
--
-- Separators and search text are plain text, never Lua patterns,
-- so "." or "%" work the way you'd expect.

local strings = {}

local function check(value, name, fn)
    if type(value) ~= "string" then
        error("strings." .. fn .. ": " .. name .. " must be text, got " .. type(value), 3)
    end
end

--- Remove spaces, tabs and new lines from both ends.
function strings.trim(text)
    check(text, "text", "trim")
    return (text:gsub("^%s+", ""):gsub("%s+$", ""))
end

--- Remove spaces from the start only.
function strings.trim_start(text)
    check(text, "text", "trim_start")
    return (text:gsub("^%s+", ""))
end

--- Remove spaces from the end only.
function strings.trim_end(text)
    check(text, "text", "trim_end")
    return (text:gsub("%s+$", ""))
end

--- Split text at every `separator` (default ","). Empty parts are kept.
function strings.split(text, separator)
    check(text, "text", "split")
    separator = separator or ","
    if separator == "" then
        local chars = {}
        for _, code in utf8.codes(text) do
            chars[#chars + 1] = utf8.char(code)
        end
        return chars
    end
    local parts, start = {}, 1
    while true do
        local from, to = text:find(separator, start, true)
        if not from then
            parts[#parts + 1] = text:sub(start)
            return parts
        end
        parts[#parts + 1] = text:sub(start, from - 1)
        start = to + 1
    end
end

--- The lines of a text (Windows or Unix line endings).
function strings.lines(text)
    check(text, "text", "lines")
    local lines = strings.split(text:gsub("\r\n", "\n"), "\n")
    if lines[#lines] == "" then
        lines[#lines] = nil
    end
    return lines
end

--- Split into words (anything between spaces).
function strings.words(text)
    check(text, "text", "words")
    local words = {}
    for word in text:gmatch("%S+") do
        words[#words + 1] = word
    end
    return words
end

function strings.starts_with(text, prefix)
    check(text, "text", "starts_with")
    return text:sub(1, #prefix) == prefix
end

function strings.ends_with(text, suffix)
    check(text, "text", "ends_with")
    return suffix == "" or text:sub(-#suffix) == suffix
end

--- True if `text` contains `part`. Pass `true` as the third argument to ignore capital letters.
function strings.contains(text, part, ignore_case)
    check(text, "text", "contains")
    if ignore_case then
        text, part = text:lower(), part:lower()
    end
    return text:find(part, 1, true) ~= nil
end

--- How many times `part` appears in `text`.
function strings.count(text, part)
    check(text, "text", "count")
    if part == "" then
        return 0
    end
    local n, start = 0, 1
    while true do
        local from, to = text:find(part, start, true)
        if not from then
            return n
        end
        n, start = n + 1, to + 1
    end
end

--- Replace every `old` with `new` (plain text). Returns the text and the number of replacements.
function strings.replace(text, old, new)
    check(text, "text", "replace")
    if old == "" then
        return text, 0
    end
    local escaped = old:gsub("[%^%$%(%)%%%.%[%]%*%+%-%?]", "%%%0")
    local safe_new = tostring(new):gsub("%%", "%%%%")
    return text:gsub(escaped, safe_new)
end

--- Number of characters (not bytes), so "ё" and "ü" count as one.
function strings.length(text)
    check(text, "text", "length")
    return utf8.len(text) or #text
end

--- Shorten long text and add "…" (or your own ending).
function strings.truncate(text, max_length, ending)
    check(text, "text", "truncate")
    ending = ending or "…"
    if strings.length(text) <= max_length then
        return text
    end
    local keep = math.max(0, max_length - strings.length(ending))
    local cut = utf8.offset(text, keep + 1) or (#text + 1)
    return text:sub(1, cut - 1) .. ending
end

local function pad(text, width, char, left)
    text = tostring(text)
    char = char or " "
    local missing = width - strings.length(text)
    if missing <= 0 then
        return text
    end
    local padding = string.rep(char, missing)
    return left and (padding .. text) or (text .. padding)
end

--- "7" -> "007" with pad_left("7", 3, "0").
function strings.pad_left(text, width, char)
    return pad(text, width, char, true)
end

function strings.pad_right(text, width, char)
    return pad(text, width, char, false)
end

--- Center text in a field of `width` characters.
function strings.center(text, width, char)
    text = tostring(text)
    local missing = width - strings.length(text)
    if missing <= 0 then
        return text
    end
    local left = math.floor(missing / 2)
    return string.rep(char or " ", left) .. text .. string.rep(char or " ", missing - left)
end

--- "hello world" -> "Hello World"
function strings.title(text)
    check(text, "text", "title")
    return (text:gsub("(%a)([%w']*)", function(first, rest)
        return first:upper() .. rest:lower()
    end))
end

--- "hello" -> "Hello"
function strings.capitalize(text)
    check(text, "text", "capitalize")
    return (text:gsub("^%l", string.upper))
end

--- "My Holiday Photos!" -> "my-holiday-photos" (for file and folder names).
function strings.slug(text)
    check(text, "text", "slug")
    local slug = text:lower():gsub("[^%w]+", "-"):gsub("^%-+", ""):gsub("%-+$", "")
    return slug
end

--- "Hello World" -> "hello_world"
function strings.snake(text)
    check(text, "text", "snake")
    return (strings.slug(text):gsub("%-", "_"))
end

--- Text read backwards (works with any language).
function strings.reverse(text)
    check(text, "text", "reverse")
    local chars = strings.split(text, "")
    local out = {}
    for i = #chars, 1, -1 do
        out[#out + 1] = chars[i]
    end
    return table.concat(out)
end

--- True for nil, "" or text with only spaces.
function strings.is_blank(text)
    return text == nil or (type(text) == "string" and text:match("^%s*$") ~= nil)
end

--- Repeat text `n` times with an optional separator in between.
function strings.rep(text, n, separator)
    return string.rep(text, n, separator)
end

--- Wrap long text into lines of at most `width` characters, breaking at spaces.
function strings.wrap(text, width)
    check(text, "text", "wrap")
    width = width or 80
    local lines, line = {}, ""
    for _, word in ipairs(strings.words(text)) do
        if line == "" then
            line = word
        elseif strings.length(line) + 1 + strings.length(word) <= width then
            line = line .. " " .. word
        else
            lines[#lines + 1] = line
            line = word
        end
    end
    if line ~= "" then
        lines[#lines + 1] = line
    end
    return table.concat(lines, "\n")
end

--- 1234567.891 -> "1,234,567.89" (separator and decimals are optional).
function strings.number(value, decimals, separator)
    decimals = decimals or 0
    separator = separator or ","
    local text = string.format("%." .. decimals .. "f", value)
    local sign, whole, fraction = text:match("^(-?)(%d+)(.*)$")
    whole = whole:reverse():gsub("(%d%d%d)", "%1" .. separator):reverse()
    if whole:sub(1, #separator) == separator then
        whole = whole:sub(#separator + 1)
    end
    return sign .. whole .. fraction
end

--- True if the text looks like an e-mail address (a quick check, not a guarantee).
function strings.is_email(text)
    return type(text) == "string" and text:match("^[%w%._%%%+%-]+@[%w%.%-]+%.%a%a+$") ~= nil
end

return strings
