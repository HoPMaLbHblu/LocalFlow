-- lf.report: build a tidy text report and save or log it.
--
--   local report = require("lf.report").new("Downloads cleanup")
--   report:line("Checked " .. #files .. " files")
--   report:table({ "File", "Size" }, rows)
--   report:save("~/Documents/Reports/cleanup.txt")   -- or report:log()

local report = {}
report.__index = report

--- A new, empty report with a title.
function report.new(title)
    local self = setmetatable({ title = title or "Report", lines = {}, counts = {} }, report)
    return self
end

--- Add a line of text (numbers and other values are turned into text).
function report:line(text)
    self.lines[#self.lines + 1] = text == nil and "" or tostring(text)
    return self
end

--- Add an empty line.
function report:blank()
    return self:line("")
end

--- Add a heading with a line under it.
function report:heading(text)
    if #self.lines > 0 then
        self:blank()
    end
    self:line(text)
    self:line(string.rep("-", utf8.len(text) or #text))
    return self
end

--- Add a bullet point.
function report:item(text)
    return self:line("  • " .. tostring(text))
end

--- Count something: report:count("moved") ... then report:summary()
function report:count(name, by)
    self.counts[name] = (self.counts[name] or 0) + (by or 1)
    return self.counts[name]
end

local function width(text)
    return utf8.len(text) or #text
end

local function pad(text, n, right_align)
    local missing = n - width(text)
    if missing <= 0 then
        return text
    end
    return right_align and (string.rep(" ", missing) .. text) or (text .. string.rep(" ", missing))
end

--- Add a table with lined-up columns. Numbers are aligned to the right.
function report:table(header, rows)
    local cells = { header }
    for _, row in ipairs(rows) do
        local texts = {}
        for i = 1, #header do
            texts[i] = row[i] == nil and "" or tostring(row[i])
        end
        cells[#cells + 1] = texts
    end
    local widths = {}
    for _, row in ipairs(cells) do
        for i, cell in ipairs(row) do
            widths[i] = math.max(widths[i] or 0, width(tostring(cell)))
        end
    end
    for r, row in ipairs(cells) do
        local parts = {}
        for i, cell in ipairs(row) do
            parts[i] = pad(tostring(cell), widths[i], r > 1 and tonumber(cell) ~= nil)
        end
        self:line((table.concat(parts, "  "):gsub("%s+$", "")))
        if r == 1 then
            local rule = {}
            for i, w in ipairs(widths) do
                rule[i] = string.rep("-", w)
            end
            self:line(table.concat(rule, "  "))
        end
    end
    return self
end

--- Add the counts collected with report:count().
function report:summary()
    local names = {}
    for name in pairs(self.counts) do
        names[#names + 1] = name
    end
    if #names == 0 then
        return self
    end
    table.sort(names)
    self:heading("Summary")
    for _, name in ipairs(names) do
        self:item(name .. ": " .. self.counts[name])
    end
    return self
end

--- The whole report as text.
function report:text()
    local header = self.title .. "\n" .. string.rep("=", width(self.title)) .. "\n" .. time.format("%Y-%m-%d %H:%M") .. "\n"
    return header .. "\n" .. table.concat(self.lines, "\n") .. "\n"
end

--- Write every line to the automation's log.
function report:log()
    log(self.title)
    for _, line in ipairs(self.lines) do
        log(line)
    end
    return self
end

--- Save the report to a file (an older file with that name goes to the Recycle Bin).
function report:save(path)
    return fs.write(path, self:text())
end

--- Add the report to the end of a file, e.g. a monthly log.
function report:append_to(path)
    return fs.append(path, self:text() .. "\n")
end

return report
