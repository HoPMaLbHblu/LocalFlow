# LocalFlow

Automate chores on your computer with small **Lua** scripts: tidy your Downloads folder, back up notes, move screenshots. Run them with one click or on a schedule. LocalFlow lives in the system tray (the icons next to the clock on the taskbar) and keeps your schedules running in the background. Available in English, Russian and German, with light and dark themes.

LocalFlow comes in two flavours that share the same engine:

- **Desktop app** (Windows): a native window with a code editor, test runs, live logs, a system tray icon, desktop notifications, "start with Windows", light/dark themes and English/Russian/German.
- **Web server**: the same features in your browser at `http://127.0.0.1:3000`, for headless machines.

## Install the desktop app

Download `LocalFlow_x.y.z_x64-setup.exe` from the [latest release](../../releases/latest) and run it. No administrator rights are needed.

After installing:

1. Click **+ New automation** and pick a template, for example *Hello world*.
2. Press **Test run** (Ctrl+Enter) to try it without saving.
3. Press **Save** (Ctrl+S). Choose a **schedule** to run it automatically.
4. Close the window whenever you like. LocalFlow keeps running in the system tray (the icons next to the clock on the taskbar; click **^** if you don't see it). Right-click its icon there and choose **Quit** to exit.

Turn on **Settings → Start with Windows** so schedules survive a reboot. **Settings → Appearance** switches between light, dark and system theme, and between English, Русский and Deutsch.

## Writing automations

```lua
automation {
    name = "Organize PDF files",

    run = function(ctx)
        local files = fs.list("~/Downloads", "*.pdf")

        for _, file in ipairs(files) do
            fs.move(file, "~/Documents/PDF/" .. fs.basename(file))
            log("Moved file: " .. file)
        end

        notify("Organized " .. #files .. " PDF file(s)")
    end
}
```

`run(ctx)` is called each time the automation runs. A script without an `automation { ... }` block also works; its top-level code simply runs.

### Lua API

| Function | What it does |
|---|---|
| `fs.list(path, pattern)` | Files in a folder whose names match a wildcard like `"*.pdf"` or `"IMG_????.jpg"` (case-insensitive). `pattern` defaults to `"*"`. |
| `fs.move(source, destination)` | Moves or renames a file. If `destination` is an existing folder, the file keeps its name. Missing parent folders are created. Refuses to overwrite. Returns the new path. |
| `fs.copy(source, destination)` | Copies a file (same destination rules; overwrites). Returns the new path. |
| `fs.exists(path)` | `true` if the file or folder exists. |
| `fs.delete(path)` | Moves a file or folder to the **Recycle Bin** (never deletes permanently). Returns `false` if it did not exist. |
| `fs.mkdir(path)` | Creates a folder and its parents. |
| `fs.basename(path)` | The file name: `"report.pdf"` for `"~/Downloads/report.pdf"`. |
| `fs.join(a, b, ...)` | Joins path parts. |
| `fs.is_dir(path)` | `true` if the path is a folder. |
| `fs.size(path)` | File size in bytes. |
| `fs.modified(path)` | When the file last changed, as a timestamp. |
| `app.open(what, args)` | Opens an app by its Start-menu name (`"Spotify"`), a file or folder with its usual app, a website, or a program by path. `args` is an optional list of options for a program. |
| `app.running(name)` | `true` if a program with that name is running (`"Discord"`, `"chrome"`; `.exe` and capitals don't matter). |
| `app.list()` | Names of the programs running now. |
| `app.shortcuts()` | Names of the apps in the Start menu, which are the names `app.open` understands. |
| `wait(seconds)` | Pauses the script (within its time limit). |
| `time.now()` | The current time as a timestamp (seconds since 1970). |
| `time.format(pattern, t)` | A timestamp as text, e.g. `time.format("%d.%m.%Y")`. Both arguments optional. |
| `time.date(t)` | A timestamp split into `year`, `month`, `day`, `hour`, `min`, `sec`, `weekday` (1 = Monday) and `yday`. |
| `time.today()` | Today's date, `"2026-09-29"`. |
| `time.days(n)`, `time.hours(n)`, `time.minutes(n)` | Durations in seconds, for comparing with timestamps. |
| `fs.read(path)` / `fs.write(path, text)` / `fs.append(path, text)` | Read, create/replace, or add to a text file. |
| `fs.rename(path, new_name)` | Renames in place; never overwrites. |
| `fs.list_dirs(folder)` | The folders inside a folder. |
| `fs.find(folder, pattern)` | Files matching `pattern` in the folder and all subfolders. |
| `fs.largest(folder, count)` | The biggest files below a folder, as `{ path, size }`. |
| `fs.hash(path)` | SHA-256 of a file. |
| `zip.create(zip_path, source)` / `zip.extract(zip_path, folder)` | Make or unpack zip archives (paths escaping the target folder are refused). |
| `security.scan(folder, options)` | File names typical of malware: words like *trojan*, *rootkit*, *keylogger* on programs, scripts and archives; fake double extensions (`invoice.pdf.exe`); Windows system program names outside the Windows folder. **Name-based only, not an antivirus.** |
| `system.disks()`, `system.disk_free(path)`, `system.memory()`, `system.cpu()`, `system.battery()`, `system.uptime()`, `system.computer_name()`, `system.user_name()`, `system.os()` | Information about the computer. |
| `clipboard.get()` / `clipboard.set(text)` | Read or set the clipboard text. |
| `ask(question, title)` | Yes/No dialog; returns `true` for Yes. |
| `sound.beep()` / `sound.play(wav_path)` | Play the Windows sound or a `.wav` file. |
| `json.encode(value, pretty)` / `json.decode(text)` | Convert between Lua tables and JSON. |
| `http.get(url, opts)` / `http.post(url, opts)` | Web requests; `opts` can hold `headers`, `json` or `body`. Returns `{ status, ok, body }`. |
| `store.get(key, default)` / `store.set(key, value)` / `store.delete(key)` | Values kept between runs of the same automation. |
| `log(message)` / `print(...)` | Writes a line to the automation's log. |
| `notify(message)` | Shows a desktop notification (desktop app) and writes a `notify` log line. |

Searches (`fs.find`, `fs.largest`, `security.scan`) stop at the script's time limit and then return what they found plus `false` as a second value. For a whole-disk scan, add the disk (e.g. `C:\`) under **Settings › Allowed folders** and raise **Settings › Script time limit** (up to 1 hour).

`ctx` contains `ctx.id`, `ctx.name`, `ctx.trigger` (`"manual"`, `"tray"` (system tray menu), `"schedule"`, `"startup"`, `"watch"` or `"test"`) and, for folder-watch runs, `ctx.file`.

Paths: `~` is your home folder, and relative paths are relative to it. `/` works as a separator on every OS.

The editor autocompletes these functions, shows what they do when you hover over them, and marks syntax errors as you type. The **Help** panel next to the editor has ready-made snippets, and **Learn** in the sidebar is a short guide to Lua for beginners.

### Ways to start an automation

| Trigger | How |
|---|---|
| **Run now** | The button in the editor, or right-click the LocalFlow icon in the system tray (next to the clock on the taskbar) › **Run** › pick an automation. |
| **Schedule** | Pick a preset or a custom cron expression (below). |
| **When LocalFlow starts** | Tick *Run when LocalFlow starts*. With **Settings › Start with Windows** this runs every time you sign in, which is perfect for opening your apps. |
| **New file in a folder** | Tick *Run when a new file appears in a folder* and choose the folder and, optionally, a pattern such as `*.pdf`. The automation runs once per new file, after it has finished downloading, with the file in `ctx.file`. |

Example: open your apps when you sign in.

```lua
automation {
    name = "Open my work apps",

    run = function(ctx)
        for _, name in ipairs({ "Spotify", "Discord", "notepad" }) do
            if not app.running(name) then
                app.open(name)
                wait(1)
            end
        end
    end
}
```

### Controlling Windows

These functions control the whole PC. Everything marked 🔒 only works when **Allow system control** is switched on for that automation (under *More triggers and permissions* in the editor). Imported automations never have it switched on.

| Function | What it does |
|---|---|
| 🔒 `shell.run(cmd, { cwd, timeout })` / `shell.powershell(script, …)` | Run a Command Prompt or PowerShell command without a window; returns `{ code, ok, output, error }`. |
| `process.list()` / `process.running(name)` / `process.wait_for(name, seconds)` | See which programs run. |
| 🔒 `process.kill(name_or_pid)` | Stop a program (Windows' own processes are protected). |
| `window.list()` / `window.find(text)` / `window.active()` | Visible windows with title, app, position and size. |
| 🔒 `window.focus/minimize/maximize/restore/close(w)` / `window.move(w, x, y, width, height)` | Arrange windows; `close` asks the app politely, like clicking X. |
| 🔒 `keyboard.press("ctrl+shift+esc")` / `keyboard.type(text)` | Key combinations (incl. media keys) and typing. |
| 🔒 `mouse.move(x, y)` / `mouse.click(x, y, button, double)` · `mouse.position()` · `screen.size()` | Mouse and screen. |
| 🔒 `system.lock()` / `system.sleep()` / `system.shutdown(delay)` / `system.restart(delay)` / `system.cancel_shutdown()` | Power; shutdown and restart always leave time to cancel (`shutdown /a`). |
| 🔒 `system.volume_up/down(steps)` / `system.mute()` / `system.brightness(percent)` / `system.set_wallpaper(path)` | Sound and display. |
| 🔒 `system.wake_at("07:30")` / `system.cancel_wake()` | Wake the PC from **sleep** at a time (a shut-down PC can only be switched on by the BIOS). Windows must allow wake timers. |
| `system.idle_seconds()` · `network.wake_on_lan(mac)` | Time since the last input; wake another PC on the network. |

More triggers, also under *More triggers and permissions*: a global **hotkey** (e.g. `Ctrl+Alt+K`), **when an app starts** or **closes** (`ctx.app`), **when the PC is idle** for some minutes, and **when a USB drive is plugged in** (`ctx.drive`; that run may read and write the drive).

### Sharing automations

Press **Export** on an automation to save it as a `.localflow` file (plain JSON with the name, code and triggers) and send it to anyone. To import one, use **📥 Import** in the sidebar, double-click the file, or drag it onto the LocalFlow window.

The import screen shows the code and what the automation can do (delete, move or write files, open programs, use the internet or the clipboard, run by itself). **Imported automations always arrive switched off**, so you can read them and do a Test run first. Only import automations from people you trust.

### Schedules

Pick a preset in the editor (every 5 minutes, every hour, weekdays at 9:00, ...) or choose **Custom** and write a cron expression with **six** fields, seconds first, in your local time zone:

```
┌ second (0-59)
│ ┌ minute (0-59)
│ │ ┌ hour (0-23)
│ │ │ ┌ day of month (1-31)
│ │ │ │ ┌ month (1-12)
│ │ │ │ │ ┌ day of week (0-6 or Sun-Sat)
0 */5 * * * *
```

Disabling an automation pauses its schedule; you can still run it manually.

### Built-in helper library

Scripts can load helpers written in Lua with `require`. They live in [`crates/core/lualib/lf/`](crates/core/lualib/lf):

| Module | What it does |
|---|---|
| `lf.strings` | trim, split, contains, replace, title, slug, pad, wrap, number formatting |
| `lf.tables` | map, filter, sort_by, group_by, unique, chunk, merge, sum, dump |
| `lf.paths` | ext, stem, with_ext, safe_name, unique file names, readable sizes |
| `lf.dates` | add days/months, days between, week numbers, "3 days ago", durations |
| `lf.retry` | try again, wait for something, "at most every 6 hours" |
| `lf.template` | fill `{placeholders}` with filters like `{size\|size}` |
| `lf.report` | build tidy text reports with aligned tables |
| `lf.test` | a tiny test framework |

```lua
local tables = require("lf.tables")
local paths = require("lf.paths")

local biggest = tables.take(tables.sort_by(fs.list("~/Downloads", "*"), fs.size, true), 3)
for _, file in ipairs(biggest) do
    log(paths.name(file) .. "  " .. paths.size_text(fs.size(file)))
end
```

Rust also provides pictures (`image.resize`, `image.convert`, `image.taken`), CSV files (`csv.read`, `csv.write`), password-locked files (`crypto.encrypt`, `crypto.decrypt`), duplicate search (`fs.duplicates`) and the PC's CPU/memory/disk/battery history (`metrics.average`, `metrics.peak`, ...). The history is recorded once a minute, kept for 30 days, and never leaves the PC.

## Your data is safe

- **Backups.** LocalFlow backs up all automations, history, logs and settings every day, before every update, before permanently deleting anything, and before restoring an older backup. **Settings › Backups** lists them and can restore any of them. Only daily backups are ever cleaned up (the newest 30 are kept).
- **Damage protection.** The database uses SQLite's write-ahead log with full sync, so a crash or power cut can't corrupt it. It is checked at every start; if it is ever damaged, LocalFlow restores the newest backup automatically and keeps the damaged copy.
- **Trash.** Deleting an automation moves it to the **Trash** with its history and logs. It can be restored, or deleted for good (a backup is made first).
- **Version history.** Every save keeps the previous version of the code and triggers; the **Versions** tab can put any of them back.
- **Your files.** Scripts never delete files permanently: `fs.delete` uses the Recycle Bin, and `fs.write`, `fs.copy` and `zip.extract` move a file they replace to the Recycle Bin first. `fs.move` and `fs.rename` never overwrite.
- **Privacy.** LocalFlow has no accounts, no telemetry and no automatic update checks. Everything stays on your computer; it only goes online when one of *your* scripts calls `http.get` or `http.post`.

## Safety

- **Sandboxed Lua.** Scripts get Lua's `string`, `table`, `math`, `utf8` and `coroutine` libraries plus the API above. `os`, `io`, `package`, `debug`, `require`, `load`, `dofile` and `loadfile` are not available.
- **Limited folders.** File functions (and watch folders) only work inside the allowed folders: your home folder by default, changeable in **Settings**. `..` and symlinks cannot be used to get out.
- **Opening apps is allowed.** `app.open` can start any installed program, file or website, because that's its job. Only run scripts you trust.
- **Limits.** Scripts are stopped after their time limit (30 seconds by default, adjustable in Settings) and may use at most 64 MB of memory.
- **Test runs are real.** A test run doesn't save the automation or its history, but file operations really happen.
- **Local only.** The desktop app opens no network ports. The web server listens on `127.0.0.1` unless you explicitly allow otherwise.

## Web server

```bash
cargo run -p localflow
```

Then open <http://127.0.0.1:3000>. Settings come from environment variables or a `.env` file (see [`.env.example`](.env.example)):

| Variable | Default | Meaning |
|---|---|---|
| `LOCALFLOW_HOST` | `127.0.0.1` | Address to listen on |
| `LOCALFLOW_PORT` | `3000` | Port |
| `LOCALFLOW_ALLOW_REMOTE` | `false` | Allow a non-loopback `LOCALFLOW_HOST` (there is no login, so be careful) |
| `DATABASE_URL` | `sqlite://localflow.db` | SQLite database |
| `LOCALFLOW_ALLOWED_DIRS` | home folder | Folders scripts may access (`;`-separated on Windows, `:` elsewhere) |
| `LOCALFLOW_SCRIPT_TIMEOUT_SECS` | `30` | Maximum run time per script |
| `RUST_LOG` | `localflow=info` | Log level |

## Development

Requirements: [Rust](https://rustup.rs), [Node.js 20+](https://nodejs.org), and on Windows the "Desktop development with C++" workload from the Visual Studio Build Tools.

```bash
cd app
npm install
npm run tauri dev      # desktop app with hot reload
npm run tauri build    # installers in target/release/bundle/
```

`npm run dev` alone opens the interface in a normal browser with made-up sample data, which is handy for working on the UI.

```bash
cargo test --workspace
```

The desktop crate embeds `app/dist`, so run `npm run build` in `app/` once before testing the whole workspace.

The Lua helper library has its own tests, written in Lua, in [`crates/core/lua_tests/`](crates/core/lua_tests). They run with the rest (`cargo test -p localflow-core --test lua_suite`). The templates run end to end in a temporary folder in `tests/templates_run.rs`.

To check every code example in the in-app guide, export them and run the guide test:

```bash
cd app
npx esbuild src/guide/content.ts --bundle --platform=node --format=esm --outfile=guide.mjs
node -e "import('./guide.mjs').then(g => console.log(JSON.stringify([...g.lessons().flatMap(l => l.blocks.filter(b => b.kind === 'code').map(b => ({ from: 'lesson ' + l.id, code: b.code, runnable: b.runnable !== false }))), ...g.apiDocs().map(d => ({ from: 'api ' + d.name, code: d.example })), ...g.snippets().map(s => ({ from: 'snippet ' + s.title, code: s.code }))])))" > guide_examples.json
cd ..
LOCALFLOW_GUIDE_EXAMPLES=app/guide_examples.json cargo test -p localflow-core --test guide_examples -- --ignored
```

### Releasing

Bump the version in `Cargo.toml` (`[workspace.package]`), `app/package.json` and `app/src-tauri/tauri.conf.json`, then push a tag:

```bash
git tag v2.0.1
git push origin v2.0.1
```

GitHub Actions ([`release.yml`](.github/workflows/release.yml)) runs the tests, builds the Windows installers and attaches them to a GitHub release.

### Project layout

```
crates/
├── core/          localflow-core: the engine, shared by both front-ends
│   ├── src/db/        SQLite models and every SQL query
│   ├── src/lua/       sandbox, Lua API (fs.*, app.*, image.*, csv.*, crypto.*, ...) and script execution
│   ├── src/metrics.rs CPU/memory/disk/battery history
│   ├── lualib/lf/     helper library written in Lua (require("lf.strings") etc.)
│   ├── lua_tests/     tests for the helper library, written in Lua
│   ├── src/scheduler/ cron jobs
│   ├── src/watcher/   folder watching
│   ├── src/service.rs LocalFlow: create/update/run/test automations, live events
│   ├── migrations/    database schema (applied automatically)
│   └── scripts/       built-in templates
└── server/        localflow: Axum + HTMX web interface
app/
├── src/           React + TypeScript interface (CodeMirror editor)
└── src-tauri/     Tauri shell: commands, tray, notifications, autostart, settings
```
