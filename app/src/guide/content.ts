// Everything the in-app helper teaches: API reference, lessons, snippets and error hints.
// This file holds the English texts; guide/ru.ts and guide/de.ts translate them.

import { language } from "../i18n";
import { code, t, tip, warning, type Block, type GuideTranslation, type HintId, type Lesson } from "./blocks";
import { ru } from "./ru";
import { de } from "./de";

export type { Block, Lesson };
// Text uses `backticks` for inline code; see renderInline() in GuideText.tsx.

// ---- API reference ---------------------------------------------------------

export interface ApiDoc {
  name: string;
  signature: string;
  summary: string;
  returns?: string;
  example: string;
}

const API_DOCS: ApiDoc[] = [
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
    summary: "Copies a file. Works like `fs.move`, but keeps the original. If the copy already exists, the old one goes to the Recycle Bin first.",
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
    summary: "Moves a file or folder to the Recycle Bin, so it can always be restored from there.",
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
    example: 'for _, file in ipairs(fs.list("~/Downloads", "*")) do\n    local mb = fs.size(file) / (1024 * 1024)\n    log(string.format("%.1f MB  %s", mb, fs.basename(file)))\nend',
  },
  {
    name: "fs.modified",
    signature: "fs.modified(path)",
    summary: "When a file was last changed, as a timestamp. Compare it with `time.now()` to find old files.",
    returns: "a timestamp (seconds since 1970)",
    example: 'for _, file in ipairs(fs.list("~/Downloads", "*")) do\n    if time.now() - fs.modified(file) > time.days(30) then\n        log(fs.basename(file) .. " is older than 30 days")\n    end\nend',
  },
  {
    name: "app.open",
    signature: "app.open(what, args)",
    summary: "Opens an app by its Start-menu name (like `\"Spotify\"`), a file or folder with its usual app, a website, or a program's full path. `args` is an optional list of extra options for a program.",
    returns: "what was opened",
    example: 'app.open("notepad")\napp.open("https://github.com")\napp.open("~/Documents")',
  },
  {
    name: "app.close",
    signature: "app.close(name)",
    summary: "Closes every window of an app the polite way, like clicking ✕, so it can still ask you to save. Needs **Allow system control**. To force a program to stop, use `process.kill`.",
    returns: "how many windows were closed",
    example: "if app.running(\"Telegram\") then\n    app.close(\"Telegram\")\nend",
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
    name: "fs.read",
    signature: "fs.read(path)",
    summary: "Reads a whole text file (up to 10 MB).",
    returns: "the file's text",
    example: 'local text = fs.read("~/Documents/todo.txt")\nlog(text)',
  },
  {
    name: "fs.write",
    signature: "fs.write(path, text)",
    summary: "Creates a text file, or replaces what's in it (the old file goes to the Recycle Bin). Missing folders are created.",
    returns: "the file's path",
    example: 'fs.write("~/Documents/hello.txt", "Hello from LocalFlow")',
  },
  {
    name: "fs.append",
    signature: "fs.append(path, text)",
    summary: "Adds text to the end of a file (creating it if needed). Good for your own log files.",
    returns: "the file's path",
    example: 'fs.append("~/Documents/diary.txt", time.today() .. ": all good\\n")',
  },
  {
    name: "fs.rename",
    signature: "fs.rename(path, new_name)",
    summary: "Renames a file or folder in the same place. `new_name` is just a name, without folders. Never overwrites.",
    returns: "the new path",
    example: 'fs.rename("~/Downloads/IMG_0001.jpg", "Holiday.jpg")',
  },
  {
    name: "fs.list_dirs",
    signature: "fs.list_dirs(folder)",
    summary: "The folders directly inside a folder.",
    returns: "a list of folder paths",
    example: 'for _, folder in ipairs(fs.list_dirs("~/Documents")) do\n    log(fs.basename(folder))\nend',
  },
  {
    name: "fs.find",
    signature: "fs.find(folder, pattern)",
    summary: "Like `fs.list`, but also looks inside every subfolder. Stops at the script's time limit; the second result is `false` then.",
    returns: "a list of file paths, and whether the search finished",
    example: 'local photos, finished = fs.find("~/Pictures", "*.jpg")\nlog(#photos .. " photos")',
  },
  {
    name: "fs.largest",
    signature: "fs.largest(folder, count)",
    summary: "The biggest files in a folder and all its subfolders, biggest first. `count` defaults to 10. To scan a whole disk, add it (like `C:\\`) to the allowed folders in Settings and raise the time limit.",
    returns: "a list of `{ path = ..., size = ... }`, and whether the search finished",
    example: 'for _, f in ipairs(fs.largest("~", 5)) do\n    log(math.floor(f.size / 1024 / 1024) .. " MB  " .. f.path)\nend',
  },
  {
    name: "fs.hash",
    signature: "fs.hash(path)",
    summary: "The SHA-256 fingerprint of a file. Two files with the same fingerprint have the same content. You can also paste it into a site like VirusTotal to check a file.",
    returns: "64 characters of hex text",
    example: 'log(fs.hash("~/Downloads/setup.exe"))',
  },
  {
    name: "zip.create",
    signature: "zip.create(zip_path, source)",
    summary: "Packs a file, a folder, or a list of them into a zip archive.",
    returns: "how many files were added",
    example: 'zip.create("~/Backups/notes.zip", "~/Documents/Notes")',
  },
  {
    name: "zip.extract",
    signature: "zip.extract(zip_path, folder)",
    summary: "Unpacks a zip archive into a folder. Files it replaces go to the Recycle Bin, and archives that try to write outside that folder are refused.",
    returns: "how many files were extracted",
    example: 'zip.extract("~/Downloads/photos.zip", "~/Pictures/Imported")',
  },
  {
    name: "security.scan",
    signature: "security.scan(folder, { recursive = true })",
    summary: "Looks for file names typical of malware: words like `trojan`, `rootkit`, `worm` or `keylogger`, fake double extensions like `invoice.pdf.exe`, and Windows system program names outside the Windows folder. It only checks names; it is not an antivirus.",
    returns: "a list of `{ path = ..., reason = ... }`, and whether the scan finished",
    example: 'for _, hit in ipairs(security.scan("~/Downloads")) do\n    log(hit.path .. ": " .. hit.reason)\nend',
  },
  {
    name: "system.disks",
    signature: "system.disks()",
    summary: "Your disks with their size and free space, in bytes.",
    returns: "a list of `{ name, mount, total, free, removable }`",
    example: 'for _, d in ipairs(system.disks()) do\n    log(d.mount .. " " .. math.floor(d.free / 1024^3) .. " GB free")\nend',
  },
  {
    name: "system.disk_free",
    signature: "system.disk_free(path)",
    summary: "Free space, in bytes, on the disk that holds `path` (your home folder if left out).",
    returns: "a number of bytes",
    example: 'log(math.floor(system.disk_free() / 1024^3) .. " GB free")',
  },
  {
    name: "system.memory",
    signature: "system.memory()",
    summary: "Memory (RAM) in bytes.",
    returns: "`{ total, used, free }`",
    example: 'local m = system.memory()\nlog(math.floor(m.used / m.total * 100) .. "% of memory in use")',
  },
  {
    name: "system.cpu",
    signature: "system.cpu()",
    summary: "How busy the processor is right now, from 0 to 100. Takes a fraction of a second to measure.",
    returns: "a percentage",
    example: 'log("CPU: " .. system.cpu() .. "%")',
  },
  {
    name: "system.battery",
    signature: "system.battery()",
    summary: "The battery level of a laptop. `nil` on a PC without a battery.",
    returns: "`{ percent, charging, plugged_in }` or `nil`",
    example: 'local b = system.battery()\nif b and b.percent < 20 and not b.plugged_in then\n    notify("Battery low: " .. b.percent .. "%")\nend',
  },
  {
    name: "system.uptime",
    signature: "system.uptime()",
    summary: "How long the computer has been on, in seconds.",
    returns: "a number of seconds",
    example: 'log("On for " .. math.floor(system.uptime() / 3600) .. " hours")',
  },
  {
    name: "system.computer_name",
    signature: "system.computer_name()",
    summary: "The name of this computer.",
    returns: "text",
    example: 'log(system.computer_name())',
  },
  {
    name: "system.user_name",
    signature: "system.user_name()",
    summary: "The name of the Windows user.",
    returns: "text",
    example: 'log("Hello, " .. system.user_name())',
  },
  {
    name: "system.os",
    signature: "system.os()",
    summary: "The Windows version.",
    returns: "text, e.g. `Windows 11 (26100)`",
    example: 'log(system.os())',
  },
  {
    name: "clipboard.get",
    signature: "clipboard.get()",
    summary: "The text currently in the clipboard (what you copied). `nil` if it isn't text.",
    returns: "text or `nil`",
    example: 'local copied = clipboard.get()\nlog(copied or "nothing copied")',
  },
  {
    name: "clipboard.set",
    signature: "clipboard.set(text)",
    summary: "Puts text into the clipboard, ready to paste.",
    example: 'clipboard.set("Copied by LocalFlow on " .. time.today())',
  },
  {
    name: "ask",
    signature: "ask(question, title)",
    summary: "Shows a Yes/No window and waits for your answer. Useful before doing something big. The waiting time counts towards the time limit.",
    returns: "`true` for Yes, `false` for No",
    example: 'if ask("Empty the Downloads/Old folder?") then\n    log("OK, emptying it")\nend',
  },
  {
    name: "sound.beep",
    signature: "sound.beep()",
    summary: "Plays the standard Windows sound.",
    example: 'sound.beep()',
  },
  {
    name: "sound.play",
    signature: "sound.play(path)",
    summary: "Plays a .wav file in the background.",
    example: 'sound.play("C:/Windows/Media/tada.wav")',
  },
  {
    name: "json.encode",
    signature: "json.encode(value, pretty)",
    summary: "Turns a Lua table into JSON text, the format most web services use. Pass `true` as `pretty` for readable output.",
    returns: "text",
    example: 'log(json.encode({ name = "LocalFlow", tags = { "fast", "local" } }))',
  },
  {
    name: "json.decode",
    signature: "json.decode(text)",
    summary: "Turns JSON text into a Lua table.",
    returns: "a Lua value",
    example: 'local data = json.decode(\'{"temperature": 21.5}\')\nlog(data.temperature)',
  },
  {
    name: "http.get",
    signature: "http.get(url, { headers = {...} })",
    summary: "Downloads a web page or asks a web service for data. Error pages don't stop the script: check `ok` or `status`.",
    returns: "`{ status, ok, body }`",
    example: 'local r = http.get("https://api.github.com/repos/lua/lua")\nif r.ok then\n    log("Stars: " .. json.decode(r.body).stargazers_count)\nend',
  },
  {
    name: "http.post",
    signature: "http.post(url, { json = {...} })",
    summary: "Sends data to a web service, as JSON (`json = table`) or plain text (`body = \"...\"`). Handy for Discord or Telegram webhooks.",
    returns: "`{ status, ok, body }`",
    example: 'http.post("https://discord.com/api/webhooks/...", {\n    json = { content = "Backup finished!" }\n})',
  },
  {
    name: "store.get",
    signature: "store.get(key, default)",
    summary: "Reads a value this automation saved in an earlier run. Returns `default` if there is none. Test runs always start empty.",
    returns: "the saved value",
    example: 'local runs = store.get("runs", 0)',
  },
  {
    name: "store.set",
    signature: "store.set(key, value)",
    summary: "Saves text, a number, `true`/`false` or a table, so the next run can read it with `store.get`. Each automation has its own store.",
    example: 'local runs = store.get("runs", 0) + 1\nstore.set("runs", runs)\nlog("This is run number " .. runs)',
  },
  {
    name: "store.delete",
    signature: "store.delete(key)",
    summary: "Forgets a saved value.",
    example: 'store.delete("runs")',
  },
  {
    name: "shell.run",
    signature: "shell.run(command, { cwd, timeout })",
    summary: "🔒 Needs **Allow system control**. Runs a command like in the Command Prompt and waits for it. Output is captured; nothing flashes on screen. Stops the command after `timeout` seconds (default 60).",
    returns: "`{ code, ok, output, error }`",
    example: "local r = shell.run(\"ipconfig\")\nlog(r.output)",
  },
  {
    name: "shell.powershell",
    signature: "shell.powershell(script, { cwd, timeout })",
    summary: "🔒 Needs **Allow system control**. Like `shell.run`, but runs a PowerShell script.",
    returns: "`{ code, ok, output, error }`",
    example: "local r = shell.powershell(\"Get-Date\")\nlog(r.output)",
  },
  {
    name: "process.list",
    signature: "process.list()",
    summary: "All running programs.",
    returns: "a list of `{ pid, name, memory_mb }`",
    example: "for _, p in ipairs(process.list()) do\n    log(p.name .. \" \" .. p.memory_mb .. \" MB\")\nend",
  },
  {
    name: "process.running",
    signature: "process.running(name)",
    summary: "Checks whether a program runs, like `app.running`.",
    returns: "`true` or `false`",
    example: "if process.running(\"chrome\") then log(\"Chrome is open\") end",
  },
  {
    name: "process.wait_for",
    signature: "process.wait_for(name, seconds)",
    summary: "Waits until a program is running, at most `seconds` (default 30).",
    returns: "`true` if it started in time",
    example: "app.open(\"notepad\")\nif process.wait_for(\"notepad\", 10) then log(\"started\") end",
  },
  {
    name: "process.kill",
    signature: "process.kill(name_or_pid)",
    summary: "🔒 Needs **Allow system control**. Stops a program at once, without asking it to save. Prefer `window.close` for apps with open documents. Windows' own programs are protected.",
    returns: "how many were stopped",
    example: "process.kill(\"notepad\")",
  },
  {
    name: "window.list",
    signature: "window.list()",
    summary: "All visible windows, front to back.",
    returns: "a list of `{ id, title, app, x, y, width, height, minimized, maximized }`",
    example: "for _, w in ipairs(window.list()) do\n    log(w.app .. \": \" .. w.title)\nend",
  },
  {
    name: "window.find",
    signature: "window.find(text)",
    summary: "The first window whose app name is `text`, or whose title contains it.",
    returns: "a window, or `nil`",
    example: "local w = window.find(\"notepad\")\nif w then log(w.title) end",
  },
  {
    name: "window.active",
    signature: "window.active()",
    summary: "The window you are working in.",
    returns: "a window, or `nil`",
    example: "local w = window.active()\nif w then log(\"In front: \" .. w.title) end",
  },
  {
    name: "window.focus",
    signature: "window.focus(window)",
    summary: "🔒 Needs **Allow system control**. Brings a window to the front. `window` can be a window from `window.find`, or text to search for.",
    example: "window.focus(\"notepad\")",
  },
  {
    name: "window.move",
    signature: "window.move(window, x, y, width, height)",
    summary: "🔒 Needs **Allow system control**. Moves and resizes a window, in pixels from the top-left corner of the screen.",
    example: "local w, h = screen.size()\nwindow.move(\"notepad\", 0, 0, w // 2, h)",
  },
  {
    name: "window.minimize",
    signature: "window.minimize(window) · window.maximize(window) · window.restore(window)",
    summary: "🔒 Needs **Allow system control**. Minimizes, maximizes or restores a window.",
    example: "window.minimize(\"discord\")",
  },
  {
    name: "window.close",
    signature: "window.close(window)",
    summary: "🔒 Needs **Allow system control**. Closes a window the polite way, like clicking X: the app can still ask you to save.",
    example: "window.close(\"notepad\")",
  },
  {
    name: "keyboard.press",
    signature: "keyboard.press(keys)",
    summary: "🔒 Needs **Allow system control**. Presses a key combination, like `\"ctrl+c\"`, `\"alt+tab\"`, `\"win+d\"` or `\"enter\"`. Media keys: `volumeup`, `volumedown`, `mute`, `playpause`, `next`, `prev`.",
    example: "keyboard.press(\"ctrl+shift+esc\")  -- Task Manager",
  },
  {
    name: "keyboard.type",
    signature: "keyboard.type(text)",
    summary: "🔒 Needs **Allow system control**. Types text into the window in front, as if you typed it.",
    example: "keyboard.type(\"Hello!\")",
  },
  {
    name: "mouse.click",
    signature: "mouse.click(x, y, button, double)",
    summary: "🔒 Needs **Allow system control**. Clicks at a position (or where the mouse is, if `x` and `y` are left out). `button` is `\"left\"`, `\"right\"` or `\"middle\"`; `double = true` double-clicks.",
    example: "mouse.click(100, 200)            -- left click\nmouse.click(100, 200, \"right\")",
  },
  {
    name: "mouse.move",
    signature: "mouse.move(x, y)",
    summary: "🔒 Needs **Allow system control**. Moves the mouse pointer.",
    example: "mouse.move(500, 300)",
  },
  {
    name: "mouse.position",
    signature: "mouse.position()",
    summary: "Where the mouse pointer is. Handy for finding coordinates for `mouse.click`.",
    returns: "`x, y`",
    example: "local x, y = mouse.position()\nlog(x .. \", \" .. y)",
  },
  {
    name: "screen.size",
    signature: "screen.size()",
    summary: "The size of the main screen in pixels.",
    returns: "`width, height`",
    example: "local width, height = screen.size()\nlog(width .. \" x \" .. height)",
  },
  {
    name: "system.lock",
    signature: "system.lock()",
    summary: "🔒 Needs **Allow system control**. Locks the PC, like pressing Win+L.",
    example: "system.lock()",
  },
  {
    name: "system.sleep",
    signature: "system.sleep()",
    summary: "🔒 Needs **Allow system control**. Puts the PC to sleep.",
    example: "system.sleep()",
  },
  {
    name: "system.shutdown",
    signature: "system.shutdown(delay) · system.restart(delay)",
    summary: "🔒 Needs **Allow system control**. Shuts down or restarts the PC after `delay` seconds (default 60). Windows shows a warning, and `system.cancel_shutdown()` (or `shutdown /a`) stops it.",
    returns: "the delay",
    example: "system.shutdown(60)  -- in one minute",
  },
  {
    name: "system.cancel_shutdown",
    signature: "system.cancel_shutdown()",
    summary: "🔒 Needs **Allow system control**. Cancels a planned shutdown or restart.",
    returns: "`true` if one was cancelled",
    example: "system.cancel_shutdown()",
  },
  {
    name: "system.volume_up",
    signature: "system.volume_up(steps) · system.volume_down(steps) · system.mute()",
    summary: "🔒 Needs **Allow system control**. Changes the volume like the volume keys (a step is usually 2%). `mute` switches sound off or back on.",
    example: "system.volume_down(10)",
  },
  {
    name: "system.brightness",
    signature: "system.brightness(percent)",
    summary: "🔒 Needs **Allow system control**. Sets the screen brightness. Works on laptop screens; most external monitors don't support it.",
    example: "system.brightness(40)",
  },
  {
    name: "system.set_wallpaper",
    signature: "system.set_wallpaper(path)",
    summary: "🔒 Needs **Allow system control**. Sets the desktop background.",
    example: "system.set_wallpaper(\"~/Pictures/mountains.jpg\")",
  },
  {
    name: "system.wake_at",
    signature: "system.wake_at(time)",
    summary: "🔒 Needs **Allow system control**. Wakes the PC from **sleep** at a time: `\"07:30\"` (next 7:30) or `\"2026-10-01 07:30\"`. It can't switch on a PC that is shut down. Windows must allow wake timers (Power Options › Sleep › Allow wake timers).",
    returns: "the wake time as text",
    example: "log(system.wake_at(\"07:30\"))",
  },
  {
    name: "system.cancel_wake",
    signature: "system.cancel_wake()",
    summary: "🔒 Needs **Allow system control**. Removes the wake timer set with `system.wake_at`.",
    example: "system.cancel_wake()",
  },
  {
    name: "system.idle_seconds",
    signature: "system.idle_seconds()",
    summary: "How long nobody has touched the keyboard or mouse.",
    returns: "seconds",
    example: "log(\"Idle for \" .. system.idle_seconds() .. \" s\")",
  },
  {
    name: "network.wake_on_lan",
    signature: "network.wake_on_lan(mac, address)",
    summary: "Wakes another PC on your network that has Wake-on-LAN switched on. `mac` is its network card's address.",
    returns: "`true` when the signal was sent",
    example: "network.wake_on_lan(\"AA:BB:CC:DD:EE:FF\")",
  },
  {
    name: "ctx.app",
    signature: "ctx.app",
    summary: "For app triggers: the program that started or closed.",
    example: "if ctx.app then log(ctx.app .. \" just started\") end",
  },
  {
    name: "ctx.drive",
    signature: "ctx.drive",
    summary: "For the USB trigger: the drive that was plugged in, like `E:\\`. The run may use that drive.",
    example: "if ctx.drive then log(\"Drive \" .. ctx.drive .. \" was plugged in\") end",
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
    summary: 'How the run started: `"manual"` (Run now), `"tray"` (the system tray menu), `"schedule"`, `"startup"`, `"watch"` (a new file) or `"test"` (Test run).',
    example: 'if ctx.trigger == "test" then\n    log("Just testing, not moving anything")\n    return\nend',
  },
  {
    name: "ctx.file",
    signature: "ctx.file",
    summary: "For automations that watch a folder: the full path of the file that just appeared. `nil` for other runs.",
    example: 'if ctx.file then\n    log("New file: " .. fs.basename(ctx.file))\nend',
  },
  {
    name: "require",
    signature: "require(\"lf.strings\")",
    summary: "Loads one of LocalFlow's built-in helper libraries: `lf.strings`, `lf.tables`, `lf.paths`, `lf.dates`, `lf.retry`, `lf.template`, `lf.report` and `lf.test`. See the lesson *Ready-made helpers*.",
    returns: "the library, as a table of functions",
    example: "local strings = require(\"lf.strings\")\nlog(strings.title(\"hello world\"))",
  },
  {
    name: "fs.duplicates",
    signature: "fs.duplicates(folder, pattern)",
    summary: "Finds files with exactly the same contents in a folder and its subfolders, biggest first. `pattern` (optional) limits it to names like `\"*.jpg\"`. Empty files are skipped.",
    returns: "a list of groups `{ size, hash, files }`, and whether the search finished",
    example: "for _, group in ipairs(fs.duplicates(\"~/Downloads\")) do\n    log(#group.files .. \" copies of \" .. group.files[1])\nend",
  },
  {
    name: "image.info",
    signature: "image.info(path)",
    summary: "Reads the size and format of a picture (PNG, JPG, WebP, GIF or BMP) without loading all of it.",
    returns: "a table with `width`, `height` and `format`",
    example: "local info = image.info(\"~/Pictures/photo.jpg\")\nlog(info.width .. \" x \" .. info.height)",
  },
  {
    name: "image.resize",
    signature: "image.resize(source, destination, max_width, max_height)",
    summary: "Saves a smaller copy of a picture that fits in `max_width` × `max_height` (the height defaults to the width). The shape is kept, and small pictures are never enlarged. The new file's extension picks the format.",
    returns: "the path of the new picture",
    example: "image.resize(\"~/Pictures/big.png\", \"~/Pictures/small.jpg\", 1200)",
  },
  {
    name: "image.convert",
    signature: "image.convert(source, destination)",
    summary: "Saves a picture in another format, chosen by the new file's extension: `.png`, `.jpg`, `.webp`, `.gif` or `.bmp`.",
    returns: "the path of the new picture",
    example: "image.convert(\"~/Downloads/photo.webp\", \"~/Downloads/photo.png\")",
  },
  {
    name: "image.taken",
    signature: "image.taken(path)",
    summary: "When a photo was taken, from the date the camera stored in it (EXIF). Screenshots and edited pictures often have none.",
    returns: "a timestamp, or `nil`",
    example: "local taken = image.taken(\"~/Pictures/IMG_0001.jpg\")\nif taken then\n    log(\"Taken on \" .. time.format(\"%d.%m.%Y\", taken))\nend",
  },
  {
    name: "csv.read",
    signature: "csv.read(path, options)",
    summary: "Reads a CSV file (Excel can save these). With a header row (the default), each row is a table by column name; pass `{ header = false }` for plain lists. `separator` can be `\";\"` or `\"tab\"`.",
    returns: "a list of rows",
    example: "for _, row in ipairs(csv.read(\"~/Documents/expenses.csv\")) do\n    log(row.date .. \": \" .. row.amount)\nend",
  },
  {
    name: "csv.write",
    signature: "csv.write(path, rows, options)",
    summary: "Saves rows as a CSV file. Rows are lists, or tables by column name when you give `{ header = { ... } }`. An older file with that name goes to the Recycle Bin.",
    returns: "the file's path",
    example: "csv.write(\"~/Documents/sizes.csv\", {\n    { name = \"a.txt\", bytes = 120 },\n}, { header = { \"name\", \"bytes\" } })",
  },
  {
    name: "crypto.encrypt",
    signature: "crypto.encrypt(source, destination, password)",
    summary: "Saves a locked copy of a file that only opens with the password (XChaCha20-Poly1305 with an Argon2 key). The password needs 8+ characters. **If you forget it, nobody can open the file.**",
    returns: "the path of the locked file",
    example: "crypto.encrypt(\"~/Documents/taxes.pdf\", \"~/Documents/taxes.pdf.locked\", \"my long password\")",
  },
  {
    name: "crypto.decrypt",
    signature: "crypto.decrypt(source, destination, password)",
    summary: "Opens a file locked with `crypto.encrypt` and saves the original contents. A wrong password gives an error and writes nothing.",
    returns: "the path of the opened file",
    example: "crypto.decrypt(\"~/Documents/taxes.pdf.locked\", \"~/Desktop/taxes.pdf\", \"my long password\")",
  },
  {
    name: "crypto.password",
    signature: "crypto.password(length)",
    summary: "Makes a random password (16 characters unless you say otherwise), without look-alike characters such as `0` and `O`.",
    returns: "the password",
    example: "log(crypto.password(20))",
  },
  {
    name: "metrics.average",
    signature: "metrics.average(name, minutes)",
    summary: "The average of `\"cpu\"`, `\"memory\"`, `\"disk\"` or `\"battery\"` (in %) over the last `minutes` (default 60), from the history LocalFlow records once a minute. `metrics.peak` and `metrics.lowest` work the same way.",
    returns: "a percentage, or `nil` if there's no history yet",
    example: "local cpu = metrics.average(\"cpu\", 30)\nif cpu and cpu > 80 then\n    notify(\"The PC has been busy for 30 minutes\")\nend",
  },
  {
    name: "metrics.recent",
    signature: "metrics.recent(minutes)",
    summary: "The recorded samples of the last `minutes`, oldest first. `metrics.latest()` gives only the newest.",
    returns: "a list of `{ at, cpu, memory, disk, battery }`",
    example: "for _, s in ipairs(metrics.recent(10)) do\n    log(time.format(\"%H:%M\", s.at) .. \"  CPU \" .. s.cpu .. \"%\")\nend",
  },
  {
    name: "time.parse",
    signature: "time.parse(text, format)",
    summary: "Turns a date written as text into a timestamp. Understands `\"2026-09-29\"` and `\"2026-09-29 14:05\"`; for other forms give a `format` like `\"%d.%m.%Y\"`.",
    returns: "a timestamp, or `nil` if the text isn't a date",
    example: "local t = time.parse(\"15.03.2026\", \"%d.%m.%Y\")\nlog(time.format(\"%A\", t))",
  },
  {
    name: "time.make",
    signature: "time.make{ year, month, day, hour, min, sec }",
    summary: "Builds a timestamp from parts. Values roll over like a calendar, so `day = 32` in January is February 1st.",
    returns: "a timestamp",
    example: "local new_year = time.make({ year = 2027, month = 1, day = 1 })\nlog(\"Days left: \" .. math.floor((new_year - time.now()) / time.days(1)))",
  },
  {
    name: "ai.ask",
    signature: "ai.ask(question, options)",
    summary: "Asks the AI (GigaChat) and waits for the answer. Needs a key in **Settings › AI**. `options` can hold `system` (instructions for how to answer), `model`, `temperature` (0 = precise, 1 = creative), `max_tokens`, and `cache_hours = 24` to reuse the answer to the exact same question instead of asking again. The text goes to Sber's servers.",
    returns: "the answer as text",
    example: "local summary = ai.ask(\"Summarize in 3 sentences:\\n\" .. fs.read(\"~/Documents/notes.txt\"))\nlog(summary)",
  },
  {
    name: "ai.chat",
    signature: "ai.chat(messages, options)",
    summary: "A whole conversation: a list of `{ role = \"user\" | \"assistant\" | \"system\", content = \"...\" }`. Use it when the AI should remember earlier questions and answers.",
    returns: "the next answer as text",
    example: "local answer = ai.chat({\n    { role = \"system\", content = \"You answer in one word.\" },\n    { role = \"user\", content = \"Capital of France?\" },\n})\nlog(answer)",
  },
  {
    name: "ai.conversation",
    signature: "ai.conversation(name, options)",
    summary: "A chat that remembers earlier questions and answers between runs of this automation (the last 20 messages, or `keep = n`). Use `chat:ask(text)`, `chat:history()` and `chat:forget()`. Test runs start with an empty memory.",
    returns: "a chat object",
    example: "local chat = ai.conversation(\"journal\", { system = \"You are my diary helper.\" })\nlog(chat:ask(\"What did I plan yesterday?\"))",
  },
  {
    name: "ai.available",
    signature: "ai.available()",
    summary: "True when a GigaChat key is saved in Settings, so a script can skip the AI part instead of failing.",
    returns: "`true` or `false`",
    example: "if not ai.available() then\n    log(\"No AI key yet, skipping the summary\")\n    return\nend",
  },
  {
    name: "desktop.set_dark_mode",
    signature: "desktop.dark_mode() / desktop.set_dark_mode(on, part)",
    summary: "Windows has two dark-mode settings: apps, and the taskbar and Start menu (\"system\"). `desktop.dark_mode()` returns both (`apps, taskbar`). `desktop.set_dark_mode(true)` changes both; add `\"apps\"` or `\"system\"` to change only one. Changing needs **Allow system control**.",
    returns: "two values: apps dark, taskbar dark",
    example: "local apps, taskbar = desktop.dark_mode()\nif time.date().hour >= 20 and not apps then\n    desktop.set_dark_mode(true, \"apps\")\nend",
  },
  {
    name: "desktop.set_transparency",
    signature: "desktop.transparency() / desktop.set_transparency(on)",
    summary: "Reads or switches Windows' transparency effects. Changing needs **Allow system control**.",
    returns: "`true` or `false`",
    example: "desktop.set_transparency(false)",
  },
  {
    name: "desktop.set_accent_color",
    signature: "desktop.accent_color() / desktop.set_accent_color(\"#0078d4\")",
    summary: "Reads or sets Windows' accent colour, as `#rrggbb`. Changing needs **Allow system control**.",
    returns: "a colour like `\"#0078d4\"`",
    example: "desktop.set_accent_color(\"#e81123\")",
  },
  {
    name: "desktop.set_wallpaper",
    signature: "desktop.wallpaper() / desktop.set_wallpaper(path, style)",
    summary: "Reads the current wallpaper, or sets a picture with a style: `\"fill\"` (default), `\"fit\"`, `\"stretch\"`, `\"center\"`, `\"tile\"` or `\"span\"`. Changing needs **Allow system control**.",
    returns: "the wallpaper's path",
    example: "desktop.set_wallpaper(\"~/Pictures/mountains.jpg\", \"fit\")",
  },
  {
    name: "system.set_volume",
    signature: "system.volume() / system.set_volume(percent)",
    summary: "Reads or sets the exact volume, 0 to 100. `system.muted()` and `system.set_mute(true)` do the same for mute. Changing needs **Allow system control**.",
    returns: "the volume in %",
    example: "if system.volume() > 60 then\n    system.set_volume(40)\nend",
  },
  {
    name: "power.set_plan",
    signature: "power.plans() / power.plan() / power.set_plan(name)",
    summary: "Lists power plans, tells which is active, or switches: `\"balanced\"`, `\"high performance\"`, `\"power saver\"` (the same names in every Windows language). `power.set_screen_off(minutes)` and `power.set_sleep(minutes)` set the timers (0 = never; add `\"plugged\"` or `\"battery\"` for just one). Changing needs **Allow system control**.",
    returns: "the plan's name",
    example: "if system.battery() and not system.battery().plugged_in then\n    power.set_plan(\"power saver\")\nend",
  },
  {
    name: "mouse.set_speed",
    signature: "mouse.speed() / mouse.set_speed(n)",
    summary: "Reads or sets the pointer speed, 1 (slow) to 20 (fast); Windows' default is 10. Changing needs **Allow system control**.",
    returns: "the speed",
    example: "mouse.set_speed(10)",
  },
  {
    name: "explorer.set_hidden_files",
    signature: "explorer.hidden_files() / explorer.set_hidden_files(show)",
    summary: "Whether File Explorer shows hidden files. `explorer.file_extensions()` and `explorer.set_file_extensions(true)` do the same for file extensions like `.pdf`. Changing needs **Allow system control**.",
    returns: "`true` when shown",
    example: "explorer.set_file_extensions(true)",
  },
  {
    name: "ctx.input",
    signature: "ctx.input",
    summary: "Data from the automation that started this one: what was passed to `automations.call`, or what the previous automation returned for **Run after** triggers. `nil` otherwise.",
    example: 'if ctx.input then\n    log("Got: " .. json.encode(ctx.input))\nend',
  },
  {
    name: "automations.call",
    signature: "automations.call(name, input)",
    summary: "Runs another saved automation as a step and waits for it. `input` (optional) arrives there as `ctx.input`. Stops this script if the step fails. Capital letters in the name don't matter.",
    returns: "whatever the step's `run` returned",
    example: 'local project = automations.call("Make project folder", { name = "Report" })\nlog(project.folder)',
  },
  {
    name: "automations.run",
    signature: "automations.run(name, input)",
    summary: "Like `automations.call`, but never stops this script. Check `ok` to see whether the step worked.",
    returns: "a table with `ok`, `error` and `result`",
    example: 'local r = automations.run("Start VPN")\nif not r.ok then\n    log("Failed: " .. r.error)\nend',
  },
  {
    name: "automations.list",
    signature: "automations.list()",
    summary: "All your saved automations, including switched-off ones, which still work as steps.",
    returns: "a list of tables with `id`, `name` and `enabled`",
    example: 'for _, a in ipairs(automations.list()) do\n    log(a.id .. ": " .. a.name)\nend',
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

const SNIPPETS: Snippet[] = [
  {
    title: "Run a command",
    description: "Run a Command Prompt command and log its output (needs system control).",
    code: "local r = shell.run(\"ipconfig\")\nlog(r.output)\n",
  },
  {
    title: "Bring a window to the front",
    description: "Find a window by app or title and focus it (needs system control).",
    code: "local w = window.find(\"notepad\")\nif w then\n    window.focus(w)\nend\n",
  },
  {
    title: "Only when I'm away",
    description: "Stop early if the keyboard or mouse was used in the last 5 minutes.",
    code: "if system.idle_seconds() < 300 then\n    log(\"Someone is using the PC, trying later\")\n    return\nend\n",
  },
  {
    title: "Remember between runs",
    description: "Count runs or keep the last value with store.",
    code: 'local count = store.get("count", 0) + 1\nstore.set("count", count)\nlog("Run number " .. count)\n',
  },
  {
    title: "Ask before doing it",
    description: "Show a Yes/No question first.",
    code: 'if not ask("Do it now?") then\n    log("Skipped")\n    return\nend\n',
  },
  {
    title: "Largest files",
    description: "List the biggest files in a folder.",
    code: 'for i, f in ipairs(fs.largest("~/Downloads", 10)) do\n    log(i .. ". " .. math.floor(f.size / 1024 / 1024) .. " MB  " .. fs.basename(f.path))\nend\n',
  },
  {
    title: "Send a webhook message",
    description: "Post a message to Discord (or another webhook).",
    code: 'local webhook = "https://discord.com/api/webhooks/..."\nlocal r = http.post(webhook, { json = { content = "Hello from LocalFlow" } })\nlog("Status: " .. r.status)\n',
  },
  {
    title: "Warn on low battery",
    description: "Notify when a laptop battery is low.",
    code: 'local b = system.battery()\nif b and b.percent < 20 and not b.plugged_in then\n    notify("Battery at " .. b.percent .. "%")\nend\n',
  },
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
    title: "Run another automation",
    description: "Use a saved automation as a step and pass it some data.",
    code: 'local result = automations.call("My other automation", { text = "hello" })\nlog("It returned: " .. tostring(result))\n',
  },
  {
    title: "Readable file sizes",
    description: "Show sizes like \"1.5 MB\" with the paths helper.",
    code: "local paths = require(\"lf.paths\")\nlog(paths.size_text(fs.size(\"~/Downloads/file.zip\")))\n",
  },
  {
    title: "Warn at most every few hours",
    description: "Send the same warning only once in a while.",
    code: "local retry = require(\"lf.retry\")\nif retry.at_most_every(\"disk-warning\", time.hours(6)) then\n    notify(\"Disk is almost full\")\nend\n",
  },
  {
    title: "Fill in a text template",
    description: "Put values into {placeholders}.",
    code: "local template = require(\"lf.template\")\nlog(template.render(\"Hi {name}, {count} new files\", { name = \"Sam\", count = 3 }))\n",
  },
  {
    title: "Stop early",
    description: "Leave the run function when there is nothing to do.",
    code: 'if not fs.exists("~/Documents/Notes") then\n    log("No notes folder, nothing to do")\n    return\nend\n',
  },
];

// ---- lessons ---------------------------------------------------------------

const LESSONS: Lesson[] = [
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
      t("Pick a **Schedule** above the editor, like *Every hour* or *Weekdays at 9:00*, then Save. LocalFlow runs the automation by itself as long as it is enabled and LocalFlow is running. When you close the window, it keeps going in the system tray (the icons next to the clock on the taskbar)."),
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
      tip("**Open everything when you sign in:** tick **Run when LocalFlow starts** above the editor, and turn on **Settings › Start with Windows**. Or right-click the LocalFlow icon in the system tray (next to the clock on the taskbar; click ^ if you don't see it) › **Run** to start any automation in one click."),
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
  {
    id: "tools",
    title: "12. More tools and sharing",
    summary: "Memory, questions, system info, the web, and sharing your automations.",
    blocks: [
      t("**Remembering things.** Variables are forgotten when a run ends. `store.set(key, value)` saves a value for the next run, and `store.get(key, default)` reads it back. (Test runs always start with an empty store.)"),
      code('local runs = store.get("runs", 0) + 1\nstore.set("runs", runs)\nlog("This automation has run " .. runs .. " times")\n'),
      t("**Asking first.** `ask(question)` shows a Yes/No window and waits for your answer:"),
      code('if ask("Say hello?") then\n    log("Hello!")\nelse\n    log("OK, maybe later")\nend\n'),
      t("**Your computer.** The `system` functions tell you about disks, memory, the battery and more:"),
      code('local gb = 1024 * 1024 * 1024\nlog("Computer: " .. system.computer_name())\nlog("Free on your disk: " .. math.floor(system.disk_free() / gb) .. " GB")\nlocal m = system.memory()\nlog("Memory in use: " .. math.floor(m.used / m.total * 100) .. "%")\n'),
      t("**Text files and JSON.** `fs.write`, `fs.append` and `fs.read` work with text files. `json.encode` and `json.decode` convert between tables and JSON, the format most web services use:"),
      code('local file = "~/Documents/LocalFlow test.json"\nfs.write(file, json.encode({ saved = time.today(), items = { "a", "b" } }))\nlocal data = json.decode(fs.read(file))\nlog("Saved on " .. data.saved .. " with " .. #data.items .. " items")\n'),
      t("**The web.** `http.get(url)` downloads a page or asks a web service for data, and `http.post(url, { json = ... })` sends data, for example to a Discord webhook:"),
      code('local r = http.get("https://api.github.com/repos/lua/lua")\nif r.ok then\n    local repo = json.decode(r.body)\n    log(repo.full_name .. " has " .. repo.stargazers_count .. " stars")\nend\n', false),
      t("**Big searches.** `fs.largest(folder, 20)` finds the biggest files and `security.scan(folder)` flags file names that look like malware. To search a whole disk, add it (for example `C:\\`) under **Settings › Allowed folders** and raise **Settings › Script time limit** to a few minutes."),
      warning("`security.scan` only looks at file *names*. It is not an antivirus: real malware can be called anything, and a match can be harmless. Check what it finds with your antivirus."),
      t("**Sharing.** Press **Export** on an automation to save it as a `.localflow` file, and send that file to a friend. They open it with **📥 Import** in the sidebar, by double-clicking it, or by dragging it onto the LocalFlow window."),
      tip("Imported automations arrive **switched off**, with a list of what they can do (delete files, open apps, use the internet, …). Read the code and do a Test run before switching one on, and only import from people you trust."),
    ],
  },
  {
    id: "control",
    title: "13. Controlling Windows",
    summary: "Commands, windows, keys, power, and more triggers.",
    blocks: [
      t("LocalFlow can do almost anything you do with Windows yourself: run commands, move windows, press keys, lock or shut down the PC. Because that is powerful, these functions only work in automations where you switch on **Allow system control** (under *More triggers and permissions* above the code). Imported automations always arrive with it off."),
      t("Looking is always allowed. These work in every automation:"),
      code("log(#process.list() .. \" programs are running\")\nlog(#window.list() .. \" windows are open\")\nlocal w, h = screen.size()\nlog(\"Screen: \" .. w .. \" x \" .. h)\nlog(\"Idle for \" .. system.idle_seconds() .. \" seconds\")\n"),
      t("**Commands.** `shell.run` runs a command like the Command Prompt and gives you the output; `shell.powershell` does the same with PowerShell:"),
      code("local r = shell.run(\"ipconfig\")\nif r.ok then\n    log(r.output)\nend\n", false),
      t("**Windows and input.** Find a window, move it, bring it to the front, or press keys and click:"),
      code("local notes = window.find(\"notepad\")\nif notes then\n    window.focus(notes)\n    keyboard.type(\"Written by LocalFlow\")\n    keyboard.press(\"ctrl+s\")\nend\n", false),
      t("**Power.** `system.lock()`, `system.sleep()`, `system.shutdown(60)` (with time to cancel), volume, brightness and wallpaper. `system.wake_at(\"07:30\")` wakes the PC from sleep in the morning."),
      tip("**More triggers.** Under *More triggers and permissions* an automation can also start from a **hotkey** (like Ctrl+Alt+K), when an **app starts or closes** (`ctx.app`), when the PC is **idle** for some minutes, or when a **USB drive** is plugged in (`ctx.drive`). Try the templates *Focus mode*, *Lock when I walk away*, *Arrange my windows* and *Import photos from a memory card*."),
      warning("Only switch on **Allow system control** for scripts you understand. A script with it can do what you can do: close programs, type into any window, or turn the PC off."),
    ],
  },
  {
    id: "combine",
    title: "14. Combining automations",
    summary: "Build something big out of small steps, like opening a VPN and then choosing its country.",
    blocks: [
      t("Big jobs are easier as several small automations, each doing one thing. A main automation then runs them in order with `automations.call(name, input)`. Every step can be tested on its own and reused by other automations."),
      t("**Step 1.** Save this as an automation called *Make project folder*. `ctx.input` holds the data it was given, and what `run` returns goes back to whoever called it:"),
      code('automation {\n    run = function(ctx)\n        local name = (ctx.input and ctx.input.name) or "New project"\n        local folder = fs.join("~/Documents/Projects", name)\n        fs.mkdir(folder)\n        log("Folder ready: " .. folder)\n        return { folder = folder }\n    end\n}\n', false),
      t("**Step 2.** Save this as *Write readme*. It gets the folder from step 1:"),
      code('automation {\n    run = function(ctx)\n        local file = fs.join(ctx.input.folder, "README.txt")\n        fs.write(file, "Started on " .. time.today())\n        log("Wrote " .. file)\n    end\n}\n', false),
      t("**The main automation** runs both, passing step 1's result to step 2. Every line the steps log appears in its log, marked with the step's name:"),
      code('local project = automations.call("Make project folder", { name = "Holiday photos" })\nautomations.call("Write readme", project)\nnotify("Project ready: " .. project.folder)\n', false),
      t("`automations.call` stops everything if a step fails, which is usually what you want. `automations.run` never stops the script. It returns `ok`, `error` and `result`, so you can decide what to do. For example, with a VPN: step *Start VPN* is just `app.open(\"MyVPN\")`, and the main automation checks it worked before choosing a country:"),
      code('local vpn = automations.run("Start VPN")\nif not vpn.ok then\n    notify("The VPN did not start: " .. vpn.error)\n    return\nend\nautomations.call("Choose VPN country", { country = "Germany" })\n', false),
      t("**Step *Choose VPN country*** types the country into the VPN window. It needs **Allow system control** switched on *in that step*. Every step keeps its own setting, and the program and window names here are examples, so use your own:"),
      code('automation {\n    run = function(ctx)\n        wait(5) -- give the VPN app time to open\n        local w = window.find("vpn") -- a word from your VPN window\'s title\n        if not w then\n            error("VPN window not found")\n        end\n        window.focus(w)\n        keyboard.type(ctx.input.country)\n        keyboard.press("enter")\n    end\n}\n', false),
      tip("**No code needed:** under *More triggers and permissions*, **Run after this automation finishes** starts an automation when another one ends, only when it worked, only when it failed, or either way. What the first one returned arrives as `ctx.input`, and its name as `ctx.previous`. Steps can also be switched off: a disabled automation never runs by itself, but can still be used as a step."),
      warning("An automation can't run itself, even through other steps, and chains stop after 8 automations in a row. All steps share the main automation's time limit, so raise **Settings › Script time limit** for long chains."),
    ],
  },
  {
    id: "helpers",
    title: "15. Ready-made helpers",
    summary: "Libraries written in Lua, pictures, spreadsheets, passwords and your PC's history.",
    blocks: [
      t("LocalFlow comes with libraries of helpers, written in Lua. Load one with `require` at the top of your script and use its functions:"),
      code("local strings = require(\"lf.strings\")\nlocal paths = require(\"lf.paths\")\nlocal dates = require(\"lf.dates\")\n\nlog(strings.title(\"monthly report\"))          -- Monthly Report\nlog(paths.ext(\"C:/Photos/cat.JPG\"))           -- jpg\nlog(paths.size_text(1536))                    -- 1.5 KB\nlog(dates.weekday_name(time.now()))\nlog(dates.duration(3725))                     -- 1 h 2 min 5 s\n"),
      t("• **lf.strings**: `trim`, `split`, `contains`, `replace`, `title`, `slug`, `pad_left`, `number`, ...\n• **lf.tables**: `map`, `filter`, `sort_by`, `group_by`, `unique`, `sum`, `dump`, ...\n• **lf.paths**: `ext`, `stem`, `with_ext`, `safe_name`, `unique`, `size_text`, ...\n• **lf.dates**: `add_days`, `add_months`, `days_between`, `week_number`, `ago`, ...\n• **lf.retry**: try again, wait for something, warn at most every few hours\n• **lf.template**, **lf.report**: fill in text, build tidy reports\n• **lf.test**: check your own code"),
      t("**Lists made easy.** `lf.tables` works with the lists that functions like `fs.list` return:"),
      code("local tables = require(\"lf.tables\")\nlocal paths = require(\"lf.paths\")\n\nlocal files = fs.list(\"~/Downloads\", \"*\")\nlocal biggest = tables.take(tables.sort_by(files, fs.size, true), 3)\nfor _, file in ipairs(biggest) do\n    log(paths.name(file) .. \"  \" .. paths.size_text(fs.size(file)))\nend\n"),
      t("**Pictures.** `image.resize` makes smaller copies, `image.convert` changes the format, and `image.taken` tells you when a photo was taken. **Spreadsheets.** `csv.read` and `csv.write` work with CSV files that Excel opens:"),
      code("local rows = {}\nfor _, file in ipairs(fs.list(\"~/Downloads\", \"*\")) do\n    rows[#rows + 1] = { name = fs.basename(file), bytes = fs.size(file) }\nend\ncsv.write(\"~/Documents/downloads.csv\", rows, { header = { \"name\", \"bytes\" } })\nlog(\"Saved \" .. #rows .. \" rows\")\n"),
      t("**Your PC's history.** While LocalFlow runs, it notes CPU, memory, disk and battery use once a minute (you'll see the chart on the overview page). Scripts can ask for averages and peaks:"),
      code("local cpu = metrics.average(\"cpu\", 60)\nif cpu then\n    log(string.format(\"CPU over the last hour: %.0f%% on average, %.0f%% at most\", cpu, metrics.peak(\"cpu\", 60)))\nelse\n    log(\"No history yet, try again in a minute\")\nend\n"),
      warning("`crypto.encrypt` locks files with a password. There is no way to get a file back without it, so keep the password somewhere safe, and keep the original until you've checked the locked copy opens."),
      tip("Try the templates *Find duplicate files*, *Sort photos by date taken*, *Weekly Downloads report*, *Monthly spending summary* and *PC health check*: each one uses these helpers, and reading them is a good way to learn."),
    ],
  },
];

// ---- friendly error hints --------------------------------------------------

const FS_NAMES = API_DOCS.filter((d) => d.name.startsWith("fs.")).map((d) => d.name);

const HINTS: { id: HintId; pattern: RegExp; hint: (m: RegExpMatchArray) => string }[] = [
  {
    id: "fieldCall", pattern: /attempt to call a nil value \(field '(\w+)'\)/,
    hint: (m) => `There is no function called \`${m[1]}\` there. Check the spelling. Available: ${FS_NAMES.map((n) => `\`${n}\``).join(", ")}.`,
  },
  {
    id: "blockedGlobal", pattern: /attempt to index a nil value \(global '(os|io|debug|package)'\)/,
    hint: (m) => `\`${m[1]}\` isn't available in LocalFlow, for safety. Use the \`fs.*\` functions to work with files.`,
  },
  {
    id: "globalCall", pattern: /attempt to call a nil value \(global '(\w+)'\)/,
    hint: (m) => `\`${m[1]}\` isn't a known function. Check the spelling (capital letters matter), or define it with \`local function ${m[1]}() ... end\` *above* the line that uses it.`,
  },
  {
    id: "nilValue", pattern: /attempt to (?:index|call) a nil value \((?:global|local|upvalue) '(\w+)'\)/,
    hint: (m) => `\`${m[1]}\` has no value here. Maybe it's misspelled, or it was never set with \`local ${m[1]} = ...\`.`,
  },
  {
    id: "concatNil", pattern: /attempt to concatenate a nil value(?: \((?:global|local|field|upvalue) '(\w+)'\))?/,
    hint: (m) => `You're joining text with \`..\`, but ${m[1] ? `\`${m[1]}\`` : "one of the values"} is \`nil\` (empty). Make sure it has a value, or use \`tostring(...)\`.`,
  },
  {
    id: "concatType", pattern: /attempt to concatenate a (table|boolean) value/,
    hint: (m) => `You can't join a ${m[1]} with \`..\` directly. Wrap it: \`tostring(value)\`${m[1] === "table" ? ", or use `#list` to get a count" : ""}.`,
  },
  {
    id: "arith", pattern: /attempt to (?:perform arithmetic|compare)/,
    hint: () => "You're doing maths or comparing with something that isn't a number (or is `nil`). Log the values to check what they are.",
  },
  { id: "missingEnd", pattern: /'end' expected/, hint: () => "An `end` is missing. Every `if`, `for`, `while` and `function` needs its own `end`." },
  { id: "missingThen", pattern: /'then' expected/, hint: () => "`if` needs `then` after the condition: `if x > 1 then ... end`. To compare, use `==`, not `=`." },
  { id: "missingDo", pattern: /'do' expected/, hint: () => "Loops need `do`: `for _, file in ipairs(files) do ... end`." },
  { id: "missingEquals", pattern: /'=' expected/, hint: () => "Lua read this as an assignment. Check for a misspelled keyword or a missing `(` in a function call." },
  { id: "unfinishedString", pattern: /unfinished string/, hint: () => "A piece of text is missing its closing quote `\"`." },
  { id: "missingBrace", pattern: /'}' expected/, hint: () => "A `{` is missing its closing `}`. Also check for missing commas between items, like `name = \"x\",`." },
  { id: "missingParen", pattern: /'\)' expected/, hint: () => "A `(` is missing its closing `)`. Also check for a missing `..` when joining text." },
  { id: "unexpectedSymbol", pattern: /unexpected symbol/, hint: () => "Lua didn't understand this line. Look for missing quotes, `..` between text, commas, or `then`/`do`." },
  { id: "accessDenied", pattern: /access denied/, hint: () => "Scripts can only use files in your allowed folders. Check the path, or add the folder in Settings." },
  { id: "notFound", pattern: /source not found|source file not found|directory not found/, hint: () => "That path doesn't exist. Remember `~` is your home folder. Use `fs.exists(path)` to check first." },
  { id: "destExists", pattern: /destination already exists/, hint: () => "`fs.move` never overwrites. Check with `fs.exists(target)` first and skip or rename." },
  { id: "timedOut", pattern: /timed out/, hint: () => "The script ran too long, usually a loop that never ends. Check your `while` loops. For big searches, raise **Settings › Script time limit**." },
  { id: "missingRun", pattern: /must define a `run/, hint: () => "Add `run = function(ctx) ... end` inside `automation { ... }`." },
  { id: "appNotFound", pattern: /could not find an app/, hint: () => 'Use the name from your Start menu, like `"Spotify"`. Run `app.shortcuts()` (or the *List my apps* template) to see every name, or give the full path to the program\'s .exe.' },
  { id: "timeLimit", pattern: /time limit/, hint: () => "Scripts must finish within their time limit, including `wait(...)`. Use shorter waits, or raise **Settings › Script time limit**." },
  { id: "stepMissing", pattern: /no automation named/, hint: () => "Use the automation's name exactly as it appears in the sidebar (capital letters don't matter), or run `automations.list()` to see every name." },
  { id: "badArgument", pattern: /bad argument/, hint: () => "A function got the wrong kind of value, often a missing argument or a number where text was expected." },
];

const TRANSLATIONS: Record<string, GuideTranslation> = { ru, de };

function translation(): GuideTranslation | undefined {
  return TRANSLATIONS[language()];
}

/** Function reference in the current language. */
export function apiDocs(): ApiDoc[] {
  const tr = translation();
  return API_DOCS.map((doc) => ({ ...doc, ...(tr?.api[doc.name] ?? {}) }));
}

/** Snippets in the current language. */
export function snippets(): Snippet[] {
  const tr = translation();
  return SNIPPETS.map((s) => ({ ...s, ...(tr?.snippets[s.title] ?? {}) }));
}

/** Lessons in the current language. */
export function lessons(): Lesson[] {
  return translation()?.lessons ?? LESSONS;
}

/** A beginner-friendly explanation for a Lua/LocalFlow error, if we recognise it. */
export function explainError(message: string): string | null {
  const tr = translation();
  const fsNames = FS_NAMES.map((n) => `\`${n}\``).join(", ");
  for (const { id, pattern, hint } of HINTS) {
    const match = message.match(pattern);
    if (match) return tr ? tr.hints[id](match, fsNames) : hint(match);
  }
  return null;
}
