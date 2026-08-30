# LocalFlow

A local automation server. You write small automations in **Lua**, and LocalFlow runs them when you click a button or on a schedule: tidying your Downloads folder, backing up notes, moving screenshots.

Rust handles the server, database, scheduler and file access. Lua scripts run in a sandbox and can only use the functions LocalFlow gives them.

## Quick start

1. Install Rust from <https://rustup.rs>.
   - On **Windows** you also need the "Desktop development with C++" workload from the Visual Studio Build Tools (the Lua interpreter is compiled from C).
2. Run:

   ```bash
   cargo run
   ```

3. Open <http://127.0.0.1:3000>.

The database (`localflow.db`) is created and migrated automatically on first start.

## Using it

1. Click **+ New automation** and pick a template (for example *Hello world*).
2. Click **Create automation**, then **Run now** on the next page.
3. The result, the log and the run history show up on the same page.
4. To run it automatically, edit it and add a **schedule** (see below).

The dashboard lists every automation with its schedule, whether it is enabled, and how its last run went.

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
| `notify(message)` | Writes a highlighted `notify` line to the log (and the server console). |

`ctx` contains `ctx.id`, `ctx.name` and `ctx.trigger` (`"manual"` or `"schedule"`).

Paths: `~` is your home folder, and relative paths are relative to it. `/` works as a separator on every OS.

More examples are in [`examples/`](examples/) and offered as templates in the app.

### Schedules

Schedules are cron expressions with **six** fields, the first being seconds, evaluated in the server's local time zone:

```
┌ second (0-59)
│ ┌ minute (0-59)
│ │ ┌ hour (0-23)
│ │ │ ┌ day of month (1-31)
│ │ │ │ ┌ month (1-12)
│ │ │ │ │ ┌ day of week (0-6 or Sun-Sat)
0 */5 * * * *
```

| Expression | Meaning |
|---|---|
| `0 */5 * * * *` | every 5 minutes |
| `0 0 * * * *` | every hour |
| `0 0 18 * * *` | every day at 18:00 |
| `0 0 9 * * Mon-Fri` | weekdays at 9:00 |

Leave the schedule empty to run the automation only by hand. Disabling an automation pauses its schedule; you can still run it manually.

## Safety

- **Local only.** LocalFlow listens on `127.0.0.1` by default. If you set a non-loopback address it falls back to `127.0.0.1` unless you also set `LOCALFLOW_ALLOW_REMOTE=true`. There is no login, so only do that on a network you trust.
- **Sandboxed Lua.** Scripts get Lua's `string`, `table`, `math`, `utf8` and `coroutine` libraries plus the API above. `os`, `io`, `package`, `debug`, `require`, `load`, `dofile` and `loadfile` are not available.
- **Limited folders.** File functions only work inside the allowed folders (your home folder by default, see `LOCALFLOW_ALLOWED_DIRS`). `..` and symlinks cannot be used to get out.
- **Limits.** Scripts are stopped after `LOCALFLOW_SCRIPT_TIMEOUT_SECS` (default 30 s) and may use at most 64 MB of memory.

## Configuration

Settings come from environment variables or a `.env` file. Copy [`.env.example`](.env.example) to `.env` to change them.

| Variable | Default | Meaning |
|---|---|---|
| `LOCALFLOW_HOST` | `127.0.0.1` | Address to listen on |
| `LOCALFLOW_PORT` | `3000` | Port |
| `LOCALFLOW_ALLOW_REMOTE` | `false` | Allow non-loopback `LOCALFLOW_HOST` |
| `DATABASE_URL` | `sqlite://localflow.db` | SQLite database |
| `LOCALFLOW_ALLOWED_DIRS` | home folder | Folders scripts may access (`;`-separated on Windows, `:` elsewhere) |
| `LOCALFLOW_SCRIPT_TIMEOUT_SECS` | `30` | Maximum run time per script |
| `RUST_LOG` | `localflow=info` | Log level |

## HTTP routes

| Method | Path | Purpose |
|---|---|---|
| GET | `/` | Dashboard |
| GET | `/automations` | List automations |
| GET | `/automations/new` | Create form (`?template=<slug>` to prefill) |
| POST | `/automations` | Create |
| GET | `/automations/{id}` | Details, recent runs and logs |
| GET | `/automations/{id}/edit` | Edit form |
| PUT | `/automations/{id}` | Update |
| POST | `/automations/{id}/run` | Run now |
| POST | `/automations/{id}/toggle` | Enable / disable |
| GET | `/automations/{id}/runs` | Execution history |
| GET | `/automations/{id}/logs` | Log viewer (auto-refreshes) |
| DELETE | `/automations/{id}` | Delete |

`POST /automations/{id}` and `POST /automations/{id}/delete` do the same as `PUT` and `DELETE`, so the pages still work if the HTMX script (loaded from unpkg.com) is unavailable.

## Project layout

```
src/
├── main.rs            startup: config, database, scheduler, server
├── lib.rs
├── api/
│   ├── routes.rs      URL → handler table
│   ├── automations.rs dashboard, create/edit/delete, run, toggle
│   └── runs.rs        run history and log viewer
├── db/
│   ├── models.rs      table rows as Rust structs
│   └── repository.rs  every SQL query
├── lua/
│   ├── engine.rs      validate and execute scripts, record runs
│   ├── sandbox.rs     restricted Lua state and folder rules
│   └── api.rs         fs.*, log, notify, automation
├── scheduler/mod.rs   cron jobs for enabled automations
├── errors.rs          AppError → HTTP error page
└── state.rs           configuration and shared state
migrations/            SQL schema (applied automatically)
templates/             HTML pages (MiniJinja)
static/style.css
examples/              example automations
tests/                 repository, Lua engine and API tests
```

## Development

```bash
cargo test
```

The tests use an in-memory database and temporary folders, so they never touch your files.
