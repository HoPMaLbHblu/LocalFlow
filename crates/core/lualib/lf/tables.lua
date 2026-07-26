-- lf.tables: working with lists and tables.
--
--   local tables = require("lf.tables")
--   local sizes = tables.map(files, fs.size)
--   local big = tables.filter(files, function(f) return fs.size(f) > 1e6 end)
--   log(tables.dump({ a = 1, b = { 2, 3 } }))
--
-- "List" means a table with items 1, 2, 3, ... like the ones fs.list returns.

local tables = {}

--- A new list with `fn(item, index)` applied to every item.
function tables.map(list, fn)
    local out = {}
    for i, item in ipairs(list) do
        out[i] = fn(item, i)
    end
    return out
end

--- Only the items for which `fn(item, index)` is true.
function tables.filter(list, fn)
    local out = {}
    for i, item in ipairs(list) do
        if fn(item, i) then
            out[#out + 1] = item
        end
    end
    return out
end

--- The items for which `fn` is false (the opposite of filter).
function tables.reject(list, fn)
    return tables.filter(list, function(item, i)
        return not fn(item, i)
    end)
end

--- Combine all items into one value: reduce({1, 2, 3}, function(sum, x) return sum + x end, 0) --> 6
function tables.reduce(list, fn, start)
    local acc = start
    for i, item in ipairs(list) do
        acc = fn(acc, item, i)
    end
    return acc
end

--- The first item for which `fn` is true, and its position.
function tables.find(list, fn)
    for i, item in ipairs(list) do
        if fn(item, i) then
            return item, i
        end
    end
    return nil
end

--- The position of `value` in the list, or nil.
function tables.index_of(list, value)
    for i, item in ipairs(list) do
        if item == value then
            return i
        end
    end
    return nil
end

function tables.contains(list, value)
    return tables.index_of(list, value) ~= nil
end

--- True if `fn` is true for at least one item.
function tables.any(list, fn)
    return tables.find(list, fn) ~= nil
end

--- True if `fn` is true for every item.
function tables.all(list, fn)
    for i, item in ipairs(list) do
        if not fn(item, i) then
            return false
        end
    end
    return true
end

--- The keys of a table, sorted when they can be.
function tables.keys(t)
    local keys = {}
    for key in pairs(t) do
        keys[#keys + 1] = key
    end
    pcall(table.sort, keys)
    return keys
end

--- The values of a table, in key order.
function tables.values(t)
    return tables.map(tables.keys(t), function(key)
        return t[key]
    end)
end

--- How many entries a table has (also works for non-lists, unlike #).
function tables.count(t)
    local n = 0
    for _ in pairs(t) do
        n = n + 1
    end
    return n
end

function tables.is_empty(t)
    return next(t) == nil
end

--- The list without repeated items (first ones kept).
function tables.unique(list)
    local seen, out = {}, {}
    for _, item in ipairs(list) do
        if not seen[item] then
            seen[item] = true
            out[#out + 1] = item
        end
    end
    return out
end

--- A new list in the opposite order.
function tables.reverse(list)
    local out = {}
    for i = #list, 1, -1 do
        out[#out + 1] = list[i]
    end
    return out
end

--- Items `from` to `to` (inclusive). Negative numbers count from the end.
function tables.slice(list, from, to)
    local n = #list
    from = from or 1
    to = to or n
    if from < 0 then from = n + from + 1 end
    if to < 0 then to = n + to + 1 end
    local out = {}
    for i = math.max(from, 1), math.min(to, n) do
        out[#out + 1] = list[i]
    end
    return out
end

--- The first `n` items.
function tables.take(list, n)
    return tables.slice(list, 1, n)
end

--- A sorted copy, ordered by `fn(item)` (smallest first; pass true to reverse).
function tables.sort_by(list, fn, descending)
    local copy = tables.slice(list)
    local keys = {}
    for _, item in ipairs(copy) do
        keys[item] = keys[item] or fn(item)
    end
    table.sort(copy, function(a, b)
        if descending then
            return keys[a] > keys[b]
        end
        return keys[a] < keys[b]
    end)
    return copy
end

--- Put items into groups by `fn(item)`: { [group] = { items... } }.
function tables.group_by(list, fn)
    local groups = {}
    for _, item in ipairs(list) do
        local key = fn(item)
        groups[key] = groups[key] or {}
        table.insert(groups[key], item)
    end
    return groups
end

--- How often each value appears: count_by(words, string.lower) --> { hello = 2, ... }
function tables.count_by(list, fn)
    local counts = {}
    for _, item in ipairs(list) do
        local key = fn and fn(item) or item
        counts[key] = (counts[key] or 0) + 1
    end
    return counts
end

--- Split a list into lists of `size` items.
function tables.chunk(list, size)
    local out = {}
    for i = 1, #list, size do
        out[#out + 1] = tables.slice(list, i, i + size - 1)
    end
    return out
end

--- One list made of several lists.
function tables.concat(...)
    local out = {}
    for _, list in ipairs({ ... }) do
        for _, item in ipairs(list) do
            out[#out + 1] = item
        end
    end
    return out
end

--- Copy of `base` with the entries of the other tables added (later ones win).
function tables.merge(base, ...)
    local out = {}
    for key, value in pairs(base) do
        out[key] = value
    end
    for _, extra in ipairs({ ... }) do
        for key, value in pairs(extra) do
            out[key] = value
        end
    end
    return out
end

--- A full copy, including tables inside tables.
function tables.copy(value, seen)
    if type(value) ~= "table" then
        return value
    end
    seen = seen or {}
    if seen[value] then
        return seen[value]
    end
    local out = {}
    seen[value] = out
    for key, item in pairs(value) do
        out[tables.copy(key, seen)] = tables.copy(item, seen)
    end
    return out
end

--- True if two values hold the same data (compares tables inside tables too).
function tables.equal(a, b)
    if type(a) ~= "table" or type(b) ~= "table" then
        return a == b
    end
    for key, value in pairs(a) do
        if not tables.equal(value, b[key]) then
            return false
        end
    end
    for key in pairs(b) do
        if a[key] == nil then
            return false
        end
    end
    return true
end

--- Sum of the numbers in a list (or of fn(item) for each item).
function tables.sum(list, fn)
    local total = 0
    for _, item in ipairs(list) do
        total = total + (fn and fn(item) or item)
    end
    return total
end

--- Average of the numbers in a list, or nil for an empty list.
function tables.average(list, fn)
    if #list == 0 then
        return nil
    end
    return tables.sum(list, fn) / #list
end

--- The item with the smallest fn(item) (or the smallest number).
function tables.min_by(list, fn)
    fn = fn or function(x) return x end
    local best, best_key
    for _, item in ipairs(list) do
        local key = fn(item)
        if best_key == nil or key < best_key then
            best, best_key = item, key
        end
    end
    return best
end

--- The item with the largest fn(item) (or the largest number).
function tables.max_by(list, fn)
    fn = fn or function(x) return x end
    local best, best_key
    for _, item in ipairs(list) do
        local key = fn(item)
        if best_key == nil or key > best_key then
            best, best_key = item, key
        end
    end
    return best
end

--- A list of numbers: range(3) --> {1, 2, 3}, range(0, 10, 5) --> {0, 5, 10}
function tables.range(from, to, step)
    if to == nil then
        from, to = 1, from
    end
    local out = {}
    for i = from, to, step or 1 do
        out[#out + 1] = i
    end
    return out
end

local function is_identifier(key)
    return type(key) == "string" and key:match("^[%a_][%w_]*$") ~= nil
end

--- Readable text for any value, handy with log(): log(tables.dump(settings))
function tables.dump(value, indent, seen)
    indent = indent or ""
    seen = seen or {}
    if type(value) == "string" then
        return string.format("%q", value)
    elseif type(value) ~= "table" then
        return tostring(value)
    elseif seen[value] then
        return "{ ... }"
    end
    seen[value] = true
    if next(value) == nil then
        return "{}"
    end
    local inner = indent .. "  "
    local lines = {}
    local n = #value
    for i = 1, n do
        lines[#lines + 1] = inner .. tables.dump(value[i], inner, seen)
    end
    for _, key in ipairs(tables.keys(value)) do
        if not (math.type(key) == "integer" and key >= 1 and key <= n) then
            local name = is_identifier(key) and key or ("[" .. tables.dump(key, inner, seen) .. "]")
            lines[#lines + 1] = inner .. name .. " = " .. tables.dump(value[key], inner, seen)
        end
    end
    return "{\n" .. table.concat(lines, ",\n") .. "\n" .. indent .. "}"
end

return tables
