# LocalFlow

Automate chores on your computer with small **Lua** scripts: tidy your Downloads folder, back up notes, move screenshots. Run them with one click or on a schedule. LocalFlow lives in the system tray and keeps your schedules running in the background.

LocalFlow comes in two flavours that share the same engine:

- **Desktop app** (Windows): a native window with a code editor, test runs, live logs, tray icon, desktop notifications and "start with Windows".
- **Web server**: the same features in your browser at `http://127.0.0.1:3000`, for headless machines.

## Install the desktop app

Download `LocalFlow_x.y.z_x64-setup.exe` from the [latest release](../../releases/latest) and run it. No administrator rights are needed.

After installing:

1. Click **+ New automation** and pick a template, for example *Hello world*.
2. Press **Test run** (Ctrl+Enter) to try it without saving.
3. Press **Save** (Ctrl+S). Choose a **schedule** to run it automatically.
4. Close the window whenever you like. LocalFlow keeps running in the tray; use **Quit** in the tray menu to exit.

Turn on **Settings → Start with Windows** so schedules survive a reboot.

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
| `fs.delete(path)` | Deletes a file or an **empty** folder. Returns `false` if it did not exist. |
| `fs.mkdir(path)` | Creates a folder and its parents. |
| `fs.basename(path)` | The file name: `"report.pdf"` for `"~/Downloads/report.pdf"`. |
| `fs.join(a, b, ...)` | Joins path parts. |
| `log(message)` / `print(...)` | Writes a line to the automation's log. |
| `notify(message)` | Shows a desktop notification (desktop app) and writes a `notify` log line. |

`ctx` contains `ctx.id`, `ctx.name` and `ctx.trigger` (`"manual"`, `"schedule"` or `"test"`).

Paths: `~` is your home folder, and relative paths are relative to it. `/` works as a separator on every OS.

The editor autocompletes these functions and marks syntax errors as you type.

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

## Safety

- **Sandboxed Lua.** Scripts get Lua's `string`, `table`, `math`, `utf8` and `coroutine` libraries plus the API above. `os`, `io`, `package`, `debug`, `require`, `load`, `dofile` and `loadfile` are not available.
- **Limited folders.** File functions only work inside the allowed folders: your home folder by default, changeable in **Settings**. `..` and symlinks cannot be used to get out.
- **Limits.** Scripts are stopped after 30 seconds and may use at most 64 MB of memory.
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
│   ├── src/lua/       sandbox, Lua API (fs.*, log, notify) and script execution
│   ├── src/scheduler/ cron jobs
│   ├── src/service.rs LocalFlow: create/update/run/test automations, live events
│   ├── migrations/    database schema (applied automatically)
│   └── scripts/       built-in templates
└── server/        localflow: Axum + HTMX web interface
app/
├── src/           React + TypeScript interface (CodeMirror editor)
└── src-tauri/     Tauri shell: commands, tray, notifications, autostart, settings
```
