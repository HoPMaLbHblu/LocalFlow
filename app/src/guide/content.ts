// Everything the in-app helper teaches: API reference, lessons, snippets and error hints.
// Text uses `backticks` for inline code; see renderInline() in GuideText.tsx.

// ---- API reference ---------------------------------------------------------

export interface ApiDoc {
  name: string;
  signature: string;
  summary: string;
  returns?: string;
  example: string;
}

export const API_DOCS: ApiDoc[] = [
  {
    name: "fs.list",
    signature: "fs.list(folder, pattern)",
    summary: "Lists the files in a folder whose names match `pattern`. `*` matches anything, `?` matches one character. Capital letters don't matter.",
    returns: "a list of full file paths",
    example: 'local files = fs.list("~/Downloads", "*.pdf")\nlog("Found " .. #files .. " PDFs")',
  },
  {
    name: "fs.move",
    signature: "fs.move(source, destination)",
    summary: "Moves or renames a file. If `destination` is an existing folder, the file keeps its name. Missing folders are created. Never overwrites an existing file.",
    returns: "the new path",
    example: 'fs.move("~/Downloads/report.pdf", "~/Documents/PDF/")',
  },
  {
    name: "fs.copy",
    signature: "fs.copy(source, destination)",
    summary: "Copies a file. Works like `fs.move`, but keeps the original and overwrites an existing copy.",
    returns: "the path of the copy",
    example: 'fs.copy("~/Documents/notes.md", "~/Backups/notes.md")',
  },
  {
    name: "fs.exists",
    signature: "fs.exists(path)",
    summary: "Checks whether a file or folder exists.",
    returns: "`true` or `false`",
    example: 'if fs.exists("~/Downloads") then\n    log("Downloads folder found")\nend',
  },
  {
    name: "fs.delete",
    signature: "fs.delete(path)",
    summary: "Deletes a file, or a folder if it is empty. Deleted files do not go to the Recycle Bin, so be careful.",
    returns: "`true` if something was deleted, `false` if it didn't exist",
    example: 'fs.delete("~/Downloads/old-installer.exe")',
  },
  {
    name: "fs.mkdir",
    signature: "fs.mkdir(folder)",
    summary: "Creates a folder, including any missing parent folders. Does nothing if it already exists.",
    returns: "the folder's path",
    example: 'fs.mkdir("~/Documents/Invoices/2026")',
  },
  {
    name: "fs.basename",
    signature: "fs.basename(path)",
    summary: "Gives the file name at the end of a path.",
    returns: "text, e.g. `report.pdf`",
    example: 'local name = fs.basename("C:/Users/me/Downloads/report.pdf")\nlog(name) -- report.pdf',
  },
  {
    name: "fs.join",
    signature: "fs.join(part1, part2, ...)",
    summary: "Joins folder and file names into one path, adding separators for you.",
    returns: "the joined path",
    example: 'local target = fs.join("~/Documents", "PDF", "report.pdf")',
  },
  {
    name: "fs.is_dir",
    signature: "fs.is_dir(path)",
    summary: "Checks whether a path is a folder (rather than a file).",
    returns: "`true` or `false`",
    example: 'if fs.is_dir("~/Documents") then\n    log("Documents is a folder")\nend',
  },
  {
    name: "fs.size",
    signature: "fs.size(path)",
    summary: "The size of a file in bytes. Divide by `1024 * 1024` for megabytes.",
    returns: "a number of bytes",
    example: 'local mb = fs.size(file) / (1024 * 1024)\nlog(string.format("%.1f MB", mb))',
  },
  {
    name: "fs.modified",
    signature: "fs.modified(path)",
    summary: "When a file was last changed, as a timestamp. Compare it with `time.now()` to find old files.",
    returns: "a timestamp (seconds since 1970)",
    example: 'if time.now() - fs.modified(file) > time.days(30) then\n    log(fs.basename(file) .. " is older than 30 days")\nend',
  },
  {
    name: "app.open",
    signature: "app.open(what, args)",
    summary: "Opens an app by its Start-menu name (like `\"Spotify\"`), a file or folder with its usual app, a website, or a program's full path. `args` is an optional list of extra options for a program.",
    returns: "what was opened",
    example: 'app.open("notepad")\napp.open("https://github.com")\napp.open("~/Documents")',
  },
  {
    name: "app.running",
    signature: "app.running(name)",
    summary: "Checks whether a program is running. Use the program's name, e.g. `\"Discord\"` or `\"chrome\"`; capital letters and `.exe` don't matter.",
    returns: "`true` or `false`",
    example: 'if not app.running("Discord") then\n    app.open("Discord")\nend',
  },
  {
    name: "app.list",
    signature: "app.list()",
    summary: "The names of all programs running right now. Handy for finding the name to use with `app.running`.",
    returns: "a list of names",
    example: 'log(table.concat(app.list(), ", "))',
  },
  {
    name: "app.shortcuts",
    signature: "app.shortcuts()",
    summary: "The names of the apps in your Start menu, which are the names `app.open` understands.",
    returns: "a list of names",
    example: 'for _, name in ipairs(app.shortcuts()) do\n    log(name)\nend',
  },
  {
    name: "wait",
    signature: "wait(seconds)",
    summary: "Pauses the script. Useful between opening apps. The whole script still has to finish within its time limit (30 seconds).",
    example: 'app.open("notepad")\nwait(2)\nlog("Notepad should be open now")',
  },
  {
    name: "time.now",
    signature: "time.now()",
    summary: "The current time as a timestamp: a number of seconds, which makes it easy to calculate with.",
    returns: "a timestamp",
    example: 'local started = time.now()',
  },
  {
    name: "time.format",
    signature: "time.format(pattern, timestamp)",
    summary: "Turns a timestamp into text. Both arguments are optional: the default pattern is `%Y-%m-%d %H:%M:%S` and the default time is now. `%Y` year, `%m` month, `%d` day, `%H` hour, `%M` minute, `%A` weekday name.",
    returns: "text, e.g. `2026-09-29 14:05:00`",
    example: 'log(time.format("%d.%m.%Y"))\nlog(time.format("%A, %H:%M"))',
  },
  {
    name: "time.date",
    signature: "time.date(timestamp)",
    summary: "Splits a time into parts: `year`, `month`, `day`, `hour`, `min`, `sec`, `weekday` (1 = Monday … 7 = Sunday) and `yday` (day of the year). Leave out the timestamp for now.",
    returns: "a table",
    example: 'local d = time.date()\nif d.weekday >= 6 then\n    log("It\'s the weekend!")\nend',
  },
  {
    name: "time.today",
    signature: "time.today()",
    summary: "Today's date as text, perfect for folder names.",
    returns: "text like `2026-09-29`",
    example: 'fs.mkdir(fs.join("~/Documents/Daily", time.today()))',
  },
  {
    name: "time.days",
    signature: "time.days(n) · time.hours(n) · time.minutes(n)",
    summary: "Converts days, hours or minutes into seconds, so you can compare them with timestamps.",
    returns: "a number of seconds",
    example: 'local week_ago = time.now() - time.days(7)',
  },
  {
    name: "log",
    signature: "log(message)",
    summary: "Writes a line to this automation's log and to the output panel. Use it to see what your script is doing.",
    example: 'log("Starting cleanup")',
  },
  {
    name: "notify",
    signature: "notify(message)",
    summary: "Shows a Windows notification (if turned on in Settings) and writes it to the log.",
    example: 'notify("Backup finished")',
  },
  {
    name: "print",
    signature: "print(value, ...)",
    summary: "Same as `log`, but accepts several values and puts a tab between them.",
    example: 'print("files:", 3)',
  },
  {
    name: "ctx.name",
    signature: "ctx.name",
    summary: "The name of the automation that is running. Available inside `run = function(ctx)`.",
    example: 'log("Hello from " .. ctx.name)',
  },
  {
    name: "ctx.trigger",
    signature: "ctx.trigger",
    summary: 'How the run started: `"manual"` (Run now), `"tray"`, `"schedule"`, `"startup"`, `"watch"` (a new file) or `"test"` (Test run).',
    example: 'if ctx.trigger == "test" then\n    log("Just testing, not moving anything")\n    return\nend',
  },
  {
    name: "ctx.file",
    signature: "ctx.file",
    summary: "For automations that watch a folder: the full path of the file that just appeared. `nil` for other runs.",
    example: 'if ctx.file then\n    log("New file: " .. fs.basename(ctx.file))\nend',
  },
  {
    name: "ctx.id",
    signature: "ctx.id",
    summary: "The automation's number. `0` during test runs.",
    example: 'log("Automation #" .. ctx.id)',
  },
];

// ---- snippets --------------------------------------------------------------

export interface Snippet {
  title: string;
  description: string;
  code: string;
}

export const SNIPPETS: Snippet[] = [
  {
    title: "Open apps if not running",
    description: "Open a list of apps, skipping ones already open.",
    code: 'for _, name in ipairs({ "notepad", "Spotify" }) do\n    if not app.running(name) then\n        app.open(name)\n        wait(1)\n    end\nend\n',
  },
  {
    title: "Handle the new file",
    description: "For folder-watching automations: use ctx.file.",
    code: 'if not ctx.file then\n    return -- not started by a new file\nend\nlog("New file: " .. fs.basename(ctx.file))\n',
  },
  {
    title: "Files older than N days",
    description: "Find files that haven't changed for a while.",
    code: 'local cutoff = time.now() - time.days(30)\nfor _, file in ipairs(fs.list("~/Downloads", "*")) do\n    if fs.modified(file) < cutoff then\n        log("Old: " .. fs.basename(file))\n    end\nend\n',
  },
  {
    title: "Folder for today",
    description: "Make a folder named after today's date.",
    code: 'local today = fs.join("~/Documents/Daily", time.today())\nfs.mkdir(today)\n',
  },
  {
    title: "Only on weekdays",
    description: "Stop early on Saturday and Sunday.",
    code: 'if time.date().weekday >= 6 then\n    log("Weekend, skipping")\n    return\nend\n',
  },
  {
    title: "Loop over files",
    description: "Do something with every matching file.",
    code: 'for _, file in ipairs(fs.list("~/Downloads", "*.pdf")) do\n    log(file)\nend\n',
  },
  {
    title: "Move without overwriting",
    description: "Only move a file if the target doesn't exist yet.",
    code: 'local target = fs.join("~/Documents/PDF", fs.basename(file))\nif fs.exists(target) then\n    log("Skipped: " .. fs.basename(file))\nelse\n    fs.move(file, target)\n    log("Moved: " .. fs.basename(file))\nend\n',
  },
  {
    title: "Count and notify",
    description: "Keep a counter and report at the end.",
    code: 'local count = 0\nfor _, file in ipairs(fs.list("~/Downloads", "*.zip")) do\n    count = count + 1\nend\nnotify("Found " .. count .. " zip files")\n',
  },
  {
    title: "Name contains a word",
    description: "Check whether a file name contains some text.",
    code: 'local name = fs.basename(file):lower()\nif name:find("invoice", 1, true) then\n    log("Invoice: " .. name)\nend\n',
  },
  {
    title: "Get the file extension",
    description: 'Turn "photo.JPG" into "jpg".',
    code: 'local ext = (fs.basename(file):match("%.([^.]+)$") or ""):lower()\n',
  },
  {
    title: "Sort files by type",
    description: "Move files into a folder named after their extension.",
    code: 'for _, file in ipairs(fs.list("~/Downloads", "*.*")) do\n    local ext = (fs.basename(file):match("%.([^.]+)$") or "other"):lower()\n    local target = fs.join("~/Downloads/Sorted", ext, fs.basename(file))\n    if not fs.exists(target) then\n        fs.move(file, target)\n    end\nend\n',
  },
  {
    title: "Dry run in tests",
    description: "Only log what would happen when you press Test run.",
    code: 'local dry_run = ctx.trigger == "test"\nif dry_run then\n    log("Would move " .. file)\nelse\n    fs.move(file, target)\nend\n',
  },
  {
    title: "Stop early",
    description: "Leave the run function when there is nothing to do.",
    code: 'if not fs.exists("~/Documents/Notes") then\n    log("No notes folder, nothing to do")\n    return\nend\n',
  },
];

// ---- lessons ---------------------------------------------------------------

export type Block =
  | { kind: "text"; text: string }
  | { kind: "code"; code: string; runnable?: boolean }
  | { kind: "tip"; text: string }
  | { kind: "warning"; text: string };

export interface Lesson {
  id: string;
  title: string;
  summary: string;
  blocks: Block[];
}

const t = (text: string): Block => ({ kind: "text", text });
const code = (source: string, runnable = true): Block => ({ kind: "code", code: source, runnable });
const tip = (text: string): Block => ({ kind: "tip", text });
const warning = (text: string): Block => ({ kind: "warning", text });

export const LESSONS: Lesson[] = [
  {
    id: "first",
    title: "1. Your first automation",
    summary: "What an automation looks like and how to try it.",
    blocks: [
      t("An automation is a small program written in Lua, an easy scripting language used in games and tools. Every automation has the same shape:"),
      code('automation {\n    name = "Say hello",\n\n    run = function(ctx)\n        log("Hello, world!")\n    end\n}\n'),
      t("`automation { ... }` describes your automation. `name` is its name, and `run` is the code that runs each time it is started. Everything between `function(ctx)` and the matching `end` is your program."),
      t("`log(...)` writes a line to the output, so you can see what your script is doing."),
      tip("Press **Open in editor** on any example on this page, then **Test run** (Ctrl+Enter). The output appears under the editor. Nothing is saved until you press Save."),
      t("Lines starting with `--` are comments. Lua ignores them; they are notes for humans:"),
      code('-- This whole line is a comment\nlog("This runs") -- this part is a comment too\n'),
    ],
  },
  {
    id: "values",
    title: "2. Values and variables",
    summary: "Text, numbers, and giving them names.",
    blocks: [
      t("A variable is a name for a value. Create one with `local`:"),
      code('local folder = "~/Downloads"\nlocal limit = 10\nlocal enabled = true\n\nlog(folder)\nlog(limit)\n'),
      t("Text (called a *string*) goes in double quotes. Numbers don't. `true` and `false` are yes/no values. `nil` means \"no value\"."),
      t("Join text together with two dots, `..`:"),
      code('local name = "report"\nlocal file = name .. ".pdf"\nlog("The file is " .. file)\n'),
      t("Numbers work as you'd expect: `+ - * /`. To change a variable, assign it again (without `local`):"),
      code('local count = 0\ncount = count + 1\ncount = count + 1\nlog("Count is " .. count)\n'),
      t("Text has handy helpers. Write them with a colon after the value:"),
      code('local name = "Holiday Photo.JPG"\nlog(name:lower())                 -- holiday photo.jpg\nlog(name:upper())                 -- HOLIDAY PHOTO.JPG\nlog(tostring(name:find("Photo")))  -- where "Photo" starts: 9\nlog(name:sub(1, 7))               -- first 7 letters: Holiday\n'),
      tip("`tostring(value)` turns anything into text, which is useful before joining it with `..`."),
    ],
  },
  {
    id: "decisions",
    title: "3. Making decisions",
    summary: "Run code only when something is true.",
    blocks: [
      t("Use `if ... then ... end` to do something only when a condition is true:"),
      code('local count = 12\n\nif count > 10 then\n    log("That\'s a lot of files")\nelseif count > 0 then\n    log("A few files")\nelse\n    log("No files")\nend\n'),
      t("Comparisons: `==` equal, `~=` not equal, `<`, `>`, `<=`, `>=`. Combine them with `and`, `or`, and `not`:"),
      code('local name = "invoice-march.pdf"\n\nif name:find("invoice") and not name:find("draft") then\n    log("A final invoice")\nend\n'),
      warning("One `=` stores a value, two `==` compares. `if count = 3 then` is an error; write `if count == 3 then`."),
      t("Checking whether a file exists before touching it is a very common decision:"),
      code('if fs.exists("~/Downloads") then\n    log("Downloads folder is there")\nelse\n    log("No Downloads folder")\nend\n'),
    ],
  },
  {
    id: "loops",
    title: "4. Lists and loops",
    summary: "Repeat work for every file.",
    blocks: [
      t("A list holds several values in curly braces. `#list` tells you how many there are:"),
      code('local fruits = { "apple", "banana", "cherry" }\nlog("I have " .. #fruits .. " fruits")\nlog("The first is " .. fruits[1])\n'),
      t("To do something for every item, loop with `for ... in ipairs(list) do ... end`:"),
      code('local fruits = { "apple", "banana", "cherry" }\n\nfor index, fruit in ipairs(fruits) do\n    log(index .. ": " .. fruit)\nend\n'),
      t("`fs.list` gives you a list of files, so this is how almost every file automation starts. Use `_` for the index when you don't need it:"),
      code('local files = fs.list("~/Downloads", "*")\nlog("Downloads has " .. #files .. " files")\n\nfor _, file in ipairs(files) do\n    log(fs.basename(file))\nend\n'),
      t("To count, use a number loop:"),
      code('for i = 1, 3 do\n    log("Round " .. i)\nend\n'),
      tip("Loops also end with `end`. Every `if`, `for` and `function` needs its own `end`."),
    ],
  },
  {
    id: "functions",
    title: "5. Your own functions",
    summary: "Name a piece of code and reuse it.",
    blocks: [
      t("A function is a named piece of code you can run many times. Define it before you use it:"),
      code('local function greet(person)\n    log("Hello, " .. person .. "!")\nend\n\ngreet("Anna")\ngreet("Ben")\n'),
      t("Functions can give a value back with `return`:"),
      code('local function extension(file)\n    return (file:match("%.([^.]+)$") or ""):lower()\nend\n\nlog(extension("photo.JPG"))   -- jpg\nlog(extension("notes.txt"))   -- txt\n'),
      t("`run = function(ctx) ... end` in your automation is itself a function. `return` inside it stops the run early."),
    ],
  },
  {
    id: "files",
    title: "6. Working with files",
    summary: "Paths, patterns, and moving files safely.",
    blocks: [
      t("Paths point to files and folders. `~` means your home folder, like `C:/Users/you`, so `~/Downloads` is your Downloads folder. You can use `/` on Windows too."),
      t("`fs.list(folder, pattern)` finds files. In the pattern, `*` means \"anything\":"),
      code('log(#fs.list("~/Downloads", "*.pdf") .. " PDFs")\nlog(#fs.list("~/Downloads", "*.zip") .. " zip files")\nlog(#fs.list("~/Downloads", "Screenshot*") .. " screenshots")\n'),
      t("A safe pattern for moving files: build the target path, skip files that are already there, and log what happened."),
      code('automation {\n    name = "Organize PDFs",\n\n    run = function(ctx)\n        for _, file in ipairs(fs.list("~/Downloads", "*.pdf")) do\n            local target = fs.join("~/Documents/PDF", fs.basename(file))\n\n            if ctx.trigger == "test" then\n                log("Would move " .. fs.basename(file))\n            elseif fs.exists(target) then\n                log("Skipped (already there): " .. fs.basename(file))\n            else\n                fs.move(file, target)\n                log("Moved " .. fs.basename(file))\n            end\n        end\n    end\n}\n'),
      warning("Test runs really change files. The example above checks `ctx.trigger == \"test\"` so that a test run only logs what it *would* do. That's a good habit for any script that moves or deletes."),
      tip("Scripts can only touch files inside your allowed folders (your home folder unless you changed it in Settings). This protects the rest of your computer."),
    ],
  },
  {
    id: "schedules",
    title: "7. Schedules and notifications",
    summary: "Run automatically and tell yourself about it.",
    blocks: [
      t("Pick a **Schedule** above the editor, like *Every hour* or *Weekdays at 9:00*, then Save. LocalFlow runs the automation by itself as long as it is enabled and LocalFlow is running (it keeps going in the tray when you close the window)."),
      t("For something the presets don't cover, choose **Custom…** and write a cron expression. It has six parts: second, minute, hour, day, month, weekday. `*` means \"every\", and `*/15` means \"every 15th\"."),
      code("-- Examples (these go in the Schedule box, not in the code):\n--   0 */15 * * * *      every 15 minutes\n--   0 30 8 * * *        every day at 8:30\n--   0 0 20 * * Sun      Sundays at 20:00\n--   0 0 9 1 * *         the 1st of every month at 9:00\n", false),
      t("Use `notify(...)` to get a Windows notification. It's useful at the end of a scheduled run:"),
      code('local moved = 3 -- pretend we moved 3 files\nif moved > 0 then\n    notify("Moved " .. moved .. " files")\nend\n'),
      tip("If a scheduled run fails, LocalFlow shows a notification too. Turn notifications on or off in Settings."),
    ],
  },
  {
    id: "apps",
    title: "8. Opening your apps",
    summary: "Start your programs and websites in one click.",
    blocks: [
      t("`app.open(...)` starts things for you. Give it the name of an app as it appears in your Start menu, a file or folder, a website, or the full path to a program:"),
      code('app.open("notepad")\n'),
      code('app.open("Spotify")                        -- Start-menu name\napp.open("https://calendar.google.com")    -- website\napp.open("~/Documents")                    -- folder\napp.open("C:/Program Files/App/app.exe")   -- program\n', false),
      t("Not sure what an app is called? `app.shortcuts()` lists every name in your Start menu:"),
      code('local names = app.shortcuts()\nlog(#names .. " apps found")\nfor i = 1, math.min(#names, 15) do\n    log(names[i])\nend\n'),
      t("To avoid opening something twice, check first with `app.running(name)`. `wait(seconds)` gives an app a moment to start before the next one:"),
      code('local apps = { "notepad" }\n\nfor _, name in ipairs(apps) do\n    if app.running(name) then\n        log(name .. " is already open")\n    else\n        app.open(name)\n        log("Opened " .. name)\n        wait(1)\n    end\nend\n'),
      tip("**Open everything when you sign in:** tick **Run when LocalFlow starts** above the editor, and turn on **Settings › Start with Windows**. Or right-click the LocalFlow tray icon › **Run** to start any automation in one click."),
      t("The *Open my work apps* template does all of this. Just change the list of names."),
    ],
  },
  {
    id: "time",
    title: "9. Dates and times",
    summary: "Today's date, file ages, and weekdays.",
    blocks: [
      t("Times in LocalFlow are *timestamps*: numbers of seconds. That makes them easy to calculate with. `time.now()` is the current time:"),
      code('local now = time.now()\nlog("Seconds since 1970: " .. now)\nlog("An hour from now: " .. time.format("%H:%M", now + time.hours(1)))\n'),
      t("`time.format(...)` turns a timestamp into readable text. `%Y` is the year, `%m` the month, `%d` the day, `%H:%M` the time and `%A` the weekday:"),
      code('log(time.format())                   -- 2026-09-29 14:05:00\nlog(time.format("%d.%m.%Y"))         -- 29.09.2026\nlog(time.format("%A"))               -- Tuesday\nlog("Today is " .. time.today())     -- 2026-09-29\n'),
      t("`time.date()` splits the time into parts you can check, such as `weekday` (1 = Monday, 7 = Sunday):"),
      code('local d = time.date()\nif d.weekday >= 6 then\n    log("Weekend!")\nelseif d.hour < 12 then\n    log("Good morning")\nelse\n    log("Good afternoon")\nend\n'),
      t("`fs.modified(file)` tells you when a file last changed. Together with `time.days(n)` you can find old files:"),
      code('local cutoff = time.now() - time.days(30)\nlocal old = 0\n\nfor _, file in ipairs(fs.list("~/Downloads", "*")) do\n    if fs.modified(file) < cutoff then\n        old = old + 1\n    end\nend\n\nlog(old .. " files in Downloads are older than 30 days")\n'),
    ],
  },
  {
    id: "watch",
    title: "10. Reacting to new files",
    summary: "Run the moment a file lands in a folder.",
    blocks: [
      t("Instead of a schedule, an automation can wait for new files. Tick **Run when a new file appears in a folder**, pick the folder (like `~/Downloads`) and optionally which files (like `*.pdf`)."),
      t("Each time a matching file appears, your automation runs once, and `ctx.file` holds the path of the new file. LocalFlow waits until the file has finished downloading."),
      code('automation {\n    name = "File new PDFs",\n\n    run = function(ctx)\n        if not ctx.file then\n            log("No new file. This runs when a PDF appears.")\n            return\n        end\n\n        local folder = fs.join("~/Documents/PDF", time.format("%Y-%m"))\n        fs.move(ctx.file, fs.join(folder, fs.basename(ctx.file)))\n        notify("Filed " .. fs.basename(ctx.file))\n    end\n}\n'),
      tip("During a **Test run** there is no new file, so `ctx.file` is `nil`. That's why the example checks for it first. To see it work for real, save it and drop a file into the folder."),
      warning("Don't move files *into* the folder you are watching. That would count as a new file and start the automation again."),
    ],
  },
  {
    id: "errors",
    title: "11. When something goes wrong",
    summary: "Reading error messages and fixing them.",
    blocks: [
      t("Errors look like `automation:5: attempt to call a nil value`. The number after `automation:` is the **line** where things went wrong. The editor also marks syntax errors with a red dot while you type."),
      t("LocalFlow adds a 💡 hint under common errors. The usual suspects:"),
      t("• **Missing `end`**: every `if`, `for` and `function` needs one.\n• **Typos**: `fs.lsit` instead of `fs.list`, or `Log` instead of `log` (capital letters matter).\n• **Joining nothing**: `\"text\" .. x` fails if `x` is `nil`. Check the variable has a value.\n• **Missing quotes**: text must be in `\"double quotes\"`."),
      t("When you're unsure what a value is, log it:"),
      code('local files = fs.list("~/Downloads", "*.pdf")\nlog("files: " .. #files)\nlog("first: " .. tostring(files[1]))\n'),
      tip("Every run is saved under **Run history** with its output and error, and **Logs** shows everything your automation has written."),
    ],
  },
];

// ---- friendly error hints --------------------------------------------------

const FS_NAMES = API_DOCS.filter((d) => d.name.startsWith("fs.")).map((d) => d.name);

const HINTS: { pattern: RegExp; hint: (m: RegExpMatchArray) => string }[] = [
  {
    pattern: /attempt to call a nil value \(field '(\w+)'\)/,
    hint: (m) => `There is no function called \`${m[1]}\` there. Check the spelling. Available: ${FS_NAMES.map((n) => `\`${n}\``).join(", ")}.`,
  },
  {
    pattern: /attempt to index a nil value \(global '(os|io|debug|package)'\)/,
    hint: (m) => `\`${m[1]}\` isn't available in LocalFlow, for safety. Use the \`fs.*\` functions to work with files.`,
  },
  {
    pattern: /attempt to call a nil value \(global '(\w+)'\)/,
    hint: (m) => `\`${m[1]}\` isn't a known function. Check the spelling (capital letters matter), or define it with \`local function ${m[1]}() ... end\` *above* the line that uses it.`,
  },
  {
    pattern: /attempt to (?:index|call) a nil value \((?:global|local|upvalue) '(\w+)'\)/,
    hint: (m) => `\`${m[1]}\` has no value here. Maybe it's misspelled, or it was never set with \`local ${m[1]} = ...\`.`,
  },
  {
    pattern: /attempt to concatenate a nil value(?: \((?:global|local|field|upvalue) '(\w+)'\))?/,
    hint: (m) => `You're joining text with \`..\`, but ${m[1] ? `\`${m[1]}\`` : "one of the values"} is \`nil\` (empty). Make sure it has a value, or use \`tostring(...)\`.`,
  },
  {
    pattern: /attempt to concatenate a (table|boolean) value/,
    hint: (m) => `You can't join a ${m[1]} with \`..\` directly. Wrap it: \`tostring(value)\`${m[1] === "table" ? ", or use `#list` to get a count" : ""}.`,
  },
  {
    pattern: /attempt to (?:perform arithmetic|compare)/,
    hint: () => "You're doing maths or comparing with something that isn't a number (or is `nil`). Log the values to check what they are.",
  },
  { pattern: /'end' expected/, hint: () => "An `end` is missing. Every `if`, `for`, `while` and `function` needs its own `end`." },
  { pattern: /'then' expected/, hint: () => "`if` needs `then` after the condition: `if x > 1 then ... end`. To compare, use `==`, not `=`." },
  { pattern: /'do' expected/, hint: () => "Loops need `do`: `for _, file in ipairs(files) do ... end`." },
  { pattern: /'=' expected/, hint: () => "Lua read this as an assignment. Check for a misspelled keyword or a missing `(` in a function call." },
  { pattern: /unfinished string/, hint: () => "A piece of text is missing its closing quote `\"`." },
  { pattern: /'}' expected/, hint: () => "A `{` is missing its closing `}`. Also check for missing commas between items, like `name = \"x\",`." },
  { pattern: /'\)' expected/, hint: () => "A `(` is missing its closing `)`. Also check for a missing `..` when joining text." },
  { pattern: /unexpected symbol/, hint: () => "Lua didn't understand this line. Look for missing quotes, `..` between text, commas, or `then`/`do`." },
  { pattern: /access denied/, hint: () => "Scripts can only use files in your allowed folders. Check the path, or add the folder in Settings." },
  { pattern: /source not found|source file not found|directory not found/, hint: () => "That path doesn't exist. Remember `~` is your home folder. Use `fs.exists(path)` to check first." },
  { pattern: /destination already exists/, hint: () => "`fs.move` never overwrites. Check with `fs.exists(target)` first and skip or rename." },
  { pattern: /timed out/, hint: () => "The script ran too long, usually a loop that never ends. Check your `while` loops." },
  { pattern: /must define a `run/, hint: () => "Add `run = function(ctx) ... end` inside `automation { ... }`." },
  { pattern: /could not find an app/, hint: () => 'Use the name from your Start menu, like `"Spotify"`. Run `app.shortcuts()` (or the *List my apps* template) to see every name, or give the full path to the program\'s .exe.' },
  { pattern: /time limit/, hint: () => "Scripts must finish within 30 seconds, including `wait(...)`. Use shorter waits." },
  { pattern: /bad argument/, hint: () => "A function got the wrong kind of value, often a missing argument or a number where text was expected." },
];

/** A beginner-friendly explanation for a Lua/LocalFlow error, if we recognise it. */
export function explainError(message: string): string | null {
  for (const { pattern, hint } of HINTS) {
    const match = message.match(pattern);
    if (match) return hint(match);
  }
  return null;
}
