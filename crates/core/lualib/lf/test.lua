-- lf.test: a tiny test framework, so you can check your own helpers.
-- LocalFlow's own Lua test suite uses it too.
--
--   local test = require("lf.test")
--   test.case("adds numbers", function()
--       test.equal(1 + 1, 2)
--   end)
--   test.finish()   -- logs a summary; the run fails if a test failed

local test = { passed = 0, failed = 0, failures = {}, current = nil }

local tables_equal
tables_equal = function(a, b)
    if type(a) ~= "table" or type(b) ~= "table" then
        return a == b
    end
    for k, v in pairs(a) do
        if not tables_equal(v, b[k]) then
            return false
        end
    end
    for k in pairs(b) do
        if a[k] == nil then
            return false
        end
    end
    return true
end

local function show(value)
    if type(value) == "string" then
        return string.format("%q", value)
    elseif type(value) == "table" then
        local ok, tables = pcall(require, "lf.tables")
        if ok then
            return (tables.dump(value):gsub("\n%s*", " "))
        end
    end
    return tostring(value)
end

local function fail(message, level)
    error({ lf_test_failure = true, message = message }, (level or 1) + 2)
end

--- Run one named test. Errors inside it are recorded, not raised.
function test.case(name, fn)
    test.current = name
    local ok, problem = pcall(fn)
    if ok then
        test.passed = test.passed + 1
    else
        test.failed = test.failed + 1
        local message = type(problem) == "table" and problem.message or tostring(problem)
        test.failures[#test.failures + 1] = name .. ": " .. message
        log("✗ " .. name .. ": " .. message)
    end
    test.current = nil
end

--- A group of tests with a shared name.
function test.group(name, fn)
    local outer = test.case
    test.case = function(case_name, case_fn)
        outer(name .. " › " .. case_name, case_fn)
    end
    local ok, problem = pcall(fn)
    test.case = outer
    if not ok then
        error(problem, 0)
    end
end

function test.equal(actual, expected, message)
    if not tables_equal(actual, expected) then
        fail((message and (message .. ": ") or "") .. "expected " .. show(expected) .. ", got " .. show(actual))
    end
end

function test.not_equal(actual, unexpected, message)
    if tables_equal(actual, unexpected) then
        fail((message and (message .. ": ") or "") .. "did not expect " .. show(unexpected))
    end
end

function test.truthy(value, message)
    if not value then
        fail(message or ("expected a true value, got " .. show(value)))
    end
end

function test.falsy(value, message)
    if value then
        fail(message or ("expected false or nil, got " .. show(value)))
    end
end

--- Numbers within `tolerance` of each other (default 0.0001).
function test.near(actual, expected, tolerance, message)
    tolerance = tolerance or 0.0001
    if type(actual) ~= "number" or math.abs(actual - expected) > tolerance then
        fail((message and (message .. ": ") or "") .. "expected about " .. show(expected) .. ", got " .. show(actual))
    end
end

--- `fn` must fail; if `contains` is given, the error must mention it.
function test.fails(fn, contains)
    local ok, problem = pcall(fn)
    if ok then
        fail("expected an error, but it worked")
    end
    local message = type(problem) == "table" and tostring(problem.message) or tostring(problem)
    if contains and not message:find(contains, 1, true) then
        fail("expected an error mentioning " .. show(contains) .. ", got " .. show(message))
    end
    return message
end

--- Log a summary; raises an error (so the run shows as failed) if any test failed.
function test.finish()
    local total = test.passed + test.failed
    if test.failed == 0 then
        log("✓ all " .. total .. " tests passed")
        return true
    end
    error(test.failed .. " of " .. total .. " tests failed:\n" .. table.concat(test.failures, "\n"), 0)
end

--- Start counting again (for scripts that run several suites).
function test.reset()
    test.passed, test.failed, test.failures = 0, 0, {}
end

return test
