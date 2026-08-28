# LocalFlow agent guide

## Scope and shared work

LocalFlow is a Rust 2021 workspace with a sandboxed Lua 5.4 automation engine, a web server, and a Tauri 2 desktop app for Windows and macOS. Inspect the current branch and working tree before editing. Preserve other sessions' uncommitted work; never stage, stash, revert, reset, or move it without explicit authorization. Task-specific restrictions override the development commands below.

## Project layout

- `Cargo.toml`: workspace members and shared versions/dependencies.
- `crates/core/`: shared `localflow-core` engine. `src/service.rs` exposes `LocalFlow` to both frontends; `src/db/` and `migrations/` own SQLite models, queries, and schema. `src/scheduler/`, `src/watcher/`, and `src/triggers.rs` handle cron, folder watching, and extra event triggers.
- `crates/core/src/lua/`: sandbox, API registration, execution, built-in module loading, and function catalog. `scripts/` holds bundled Lua templates; `lualib/lf/` holds Lua helpers; `lua_tests/` holds their Lua suites; `tests/` holds Rust integration tests.
- Other core modules: `backup.rs` handles recovery; `sharing.rs` handles imports/exports and risks; `metrics.rs` records resource history; `ai.rs`, `messaging.rs`, and `remote.rs` support AI and messaging. `src/dota/` implements the Dota companion; `links.rs` and `appdata.rs` implement link sets and small data files.
- `crates/server/`: `localflow` Axum web server using the core, MiniJinja templates in `templates/`, static assets in `static/`, and HTTP handlers in `src/api/`.
- `app/src-tauri/`: desktop Rust backend: Tauri commands, tray, notifications, autostart, hotkeys, settings, secrets, and Dota integration. `tauri.conf.json` connects Vite and the packaged frontend.
- `app/src/`: React/TypeScript UI with CodeMirror; `components/` contains screens/widgets, `api.ts` wraps backend calls, `i18n/` contains translations, and `guide/` contains Lua lessons, API docs, snippets, and hints.
- `.github/workflows/`: CI and release workflows. `target/` and `app/dist/` are build outputs.

## Build and validation

Run these only when the current task permits builds and tests. Requirements are Rust, Node.js, and Windows C++ build tools (see `README.md`).

- In `app/`, install dependencies with `npm ci`, then build the desktop app with `npx tauri build`. Tauri runs `npm run build` for the frontend automatically.
- On Windows, prefer setting `CARGO_TARGET_DIR` to a short directory outside the repository to reduce long-path problems. This is contributor guidance, not an existing repository setting; choose the location locally without recording personal paths here.
- At the repository root: `cargo test --workspace`. Build the frontend first with `npm run build` in `app/` because the desktop crate embeds `app/dist` (also the CI order).
- Lua suites in `crates/core/lua_tests/` run through `crates/core/tests/lua_suite.rs`: `cargo test -p localflow-core --test lua_suite`. Add each new suite both as a Rust test and to `every_suite_is_listed`.
- In `app/`: `npx tsc --noEmit -p .` checks the strict TypeScript project without emitting files.
- `crates/core/tests/templates_run.rs` exercises templates; `guide_examples.rs` checks exported guide examples (the export recipe is in `README.md`).

## Templates and Lua APIs

- Templates live in `crates/core/scripts/*.lua` and are embedded in `EXAMPLES` in `crates/core/src/lua/mod.rs`, with a slug, category, descriptive metadata, and trigger/permission defaults.
- Every template must pass `every_template_passes_the_check` in `src/lua/catalog.rs`. Template hotkeys must be unique; `template_hotkeys_are_unique` in `tests/lua_engine.rs` enforces this case-insensitively.
- Register APIs through `src/lua/api.rs`; register built-in Lua helpers in `src/lua/lualib.rs`. Preserve the sandbox and allowed-folder checks.
- Powerful functions require **Allow system control** at runtime. Add new privileged calls to `SYSTEM_CONTROL` in `crates/core/src/ai.rs` and the appropriate risk detection list in `crates/core/src/sharing.rs`. Detection lists supplement runtime permission enforcement.
- New template categories also need `CATEGORIES` in `app/src/components/TemplatePicker.tsx` and translated category names; provide translated template titles/descriptions.

## UI strings and documentation

Add UI keys to `app/src/i18n/en.ts`, `ru.ts`, and `de.ts`. English defines `Key`; Russian and German use `Dictionary = Record<Key, string>` and must contain every key.

Keep Lua API documentation synchronized in `app/src/guide/content.ts`, its `guide/ru.ts` and `guide/de.ts` translations, and the API/helper tables in `README.md`.

## Data safety, secrets, and real-PC tests

- Never permanently destroy user files: use the Recycle Bin and refuse the operation if recycling fails (`src/lua/recycle.rs`). Preserve replaced files; moves/renames must not silently overwrite.
- Preserve automation trash, version history, and backups. `src/service.rs` backs up before permanent automation deletion; `src/backup.rs` retains damaged databases during recovery and only prunes old daily backups.
- Use temporary-file writes followed by rename for small data updates, preserving a backup, as in `src/appdata.rs::write_safely`. Preserve damaged files for diagnosis; do not claim this helper guarantees durability under every failure.
- Store the GigaChat key, Telegram token, Discord webhook, and OpenDota key only in the OS keyring, never in files. Desktop implementations are in `app/src-tauri/src/ai.rs`, `bots.rs`, and `dota.rs`; scripts must not expose the secrets.
- Test runs execute real operations. Prefer isolated temporary folders and the test recycle destination. Live tests such as `tests/tools_live.rs` are ignored because they can access the screen/network or open apps. Owner-required rule: any test on the owner's real PC that changes a setting must snapshot it and restore it, including on failure.

## Commit policy

When commits are authorized, use the repository owner's GitHub no-reply author identity, consistent with existing history. Resolve it locally from owner-authored commits; do not copy usernames or email addresses into this file. Do not add AI co-author lines.
