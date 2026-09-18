-- lf.paths: taking file paths apart and building new ones.
--
--   local paths = require("lf.paths")
--   paths.ext("C:/Photos/cat.JPG")          --> "jpg"
--   paths.stem("report.final.pdf")          --> "report.final"
--   paths.unique("~/Documents/notes.txt")   --> "~/Documents/notes (2).txt" if notes.txt exists
--
-- These only work with the text of a path; only `unique` and `ensure_dir`
-- look at the disk (through the normal fs functions).

local paths = {}

--- Windows' long-path form: \\?\C:\x -> C:/x and \\?\UNC\server\x -> //server/x
local function strip_long_prefix(path)
    local p = tostring(path):gsub("\\", "/")
    if p:sub(1, 8):upper() == "//?/UNC/" then
        return "//" .. p:sub(9)
    elseif p:sub(1, 4) == "//?/" then
        return p:sub(5)
    end
    return p
end

--- Forward slashes everywhere, no slash at the end (except a bare drive like "C:/").
function paths.normalize(path)
    local p = strip_long_prefix(path)
    local network = p:sub(1, 2) == "//"
    p = p:gsub("/+", "/")
    if network then
        p = "/" .. p
    end
    if #p > 1 and p:sub(-1) == "/" and not p:match("^%a:/$") then
        p = p:sub(1, -2)
    end
    return p
end

--- The last part of a path: "C:/a/b.txt" --> "b.txt"
function paths.name(path)
    return paths.normalize(path):match("([^/]*)$")
end

--- The folder a path is in: "C:/a/b.txt" --> "C:/a"
function paths.dir(path)
    local p = paths.normalize(path)
    local dir = p:match("^(.*)/[^/]*$")
    if dir == nil then
        return "."
    end
    if dir == "" then
        return "/"
    end
    if dir:match("^%a:$") then
        return dir .. "/"
    end
    return dir
end

--- The extension in lower case, without the dot: "photo.JPG" --> "jpg" ("" if none).
function paths.ext(path)
    local name = paths.name(path)
    local ext = name:match("^.+%.([^%.]+)$")
    return ext and ext:lower() or ""
end

--- The name without its extension: "report.final.pdf" --> "report.final"
function paths.stem(path)
    local name = paths.name(path)
    return name:match("^(.+)%.[^%.]+$") or name
end

--- The same path with another extension: with_ext("a/b.png", "jpg") --> "a/b.jpg"
function paths.with_ext(path, ext)
    ext = ext:gsub("^%.", "")
    local dir = paths.dir(path)
    local name = paths.stem(path) .. (ext ~= "" and ("." .. ext) or "")
    return dir == "." and name or paths.join(dir, name)
end

--- The same folder, a different file name.
function paths.with_name(path, name)
    local dir = paths.dir(path)
    return dir == "." and name or paths.join(dir, name)
end

--- Join parts with "/": join("C:/Users", "me", "file.txt")
function paths.join(...)
    local parts = {}
    for i, part in ipairs({ ... }) do
        part = strip_long_prefix(part)
        if i > 1 then
            part = part:gsub("^/+", "")
        end
        if part ~= "" then
            parts[#parts + 1] = part:gsub("/+$", "")
        end
    end
    local joined = table.concat(parts, "/")
    return joined == "" and "." or joined
end

--- The parts of a path: "C:/a/b.txt" --> { "C:", "a", "b.txt" }
function paths.parts(path)
    local out = {}
    for part in paths.normalize(path):gmatch("[^/]+") do
        out[#out + 1] = part
    end
    return out
end

--- True if the file name ends in one of the given extensions: has_ext(f, "jpg", "png")
function paths.has_ext(path, ...)
    local ext = paths.ext(path)
    for _, wanted in ipairs({ ... }) do
        if ext == wanted:lower():gsub("^%.", "") then
            return true
        end
    end
    return false
end

-- Characters Windows doesn't allow in file names, and names it reserves.
local RESERVED = {
    CON = true, PRN = true, AUX = true, NUL = true,
    COM1 = true, COM2 = true, COM3 = true, COM4 = true, COM5 = true, COM6 = true, COM7 = true, COM8 = true, COM9 = true,
    LPT1 = true, LPT2 = true, LPT3 = true, LPT4 = true, LPT5 = true, LPT6 = true, LPT7 = true, LPT8 = true, LPT9 = true,
}

--- Make any text safe to use as a file name: safe_name('Q3: "final"?') --> "Q3 - final"
function paths.safe_name(text, replacement)
    replacement = replacement or " - "
    -- Mark bad characters first, so a run of them (even with spaces between) becomes one replacement.
    local name = tostring(text)
        :gsub('[<>:"/\\|%?%*%c]', "\1")
        :gsub("%s*\1[\1%s]*", function()
            return replacement
        end)
        :gsub("%s+", " ")
        :gsub("^[%s%.%-]+", "")
        :gsub("[%s%.%-]+$", "")
    if name == "" then
        name = "unnamed"
    end
    if RESERVED[name:upper():match("^[^%.]*")] then
        name = "_" .. name
    end
    if #name > 200 then
        name = name:sub(1, 200)
    end
    return name
end

--- A path that doesn't exist yet: adds " (2)", " (3)", ... before the extension.
function paths.unique(path)
    if not fs.exists(path) then
        return path
    end
    local ext = paths.ext(path)
    local stem = paths.stem(path)
    local dir = paths.dir(path)
    for n = 2, 10000 do
        local name = stem .. " (" .. n .. ")" .. (ext ~= "" and ("." .. paths.name(path):match("%.([^%.]+)$")) or "")
        local candidate = dir == "." and name or paths.join(dir, name)
        if not fs.exists(candidate) then
            return candidate
        end
    end
    error("paths.unique: too many files named like " .. path, 2)
end

--- Create the folder if needed and return it.
function paths.ensure_dir(path)
    if not fs.exists(path) then
        fs.mkdir(path)
    end
    return path
end

--- Readable size: 1536 --> "1.5 KB"
function paths.size_text(bytes)
    local units = { "bytes", "KB", "MB", "GB", "TB" }
    local value, unit = bytes, 1
    while value >= 1024 and unit < #units do
        value, unit = value / 1024, unit + 1
    end
    if unit == 1 then
        return string.format("%d %s", value, units[unit])
    end
    return (string.format("%.1f", value):gsub("%.0$", "")) .. " " .. units[unit]
end

return paths
