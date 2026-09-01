-- Automations written by the user.
CREATE TABLE IF NOT EXISTS automations (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL,
    description TEXT    NOT NULL DEFAULT '',
    lua_code    TEXT    NOT NULL,
    schedule    TEXT,                          -- cron expression, NULL = manual only
    enabled     INTEGER NOT NULL DEFAULT 1,    -- 0 = disabled, 1 = enabled
    created_at  TEXT    NOT NULL,              -- RFC 3339 timestamps (UTC)
    updated_at  TEXT    NOT NULL
);

-- One row per execution of an automation.
CREATE TABLE IF NOT EXISTS automation_runs (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    automation_id INTEGER NOT NULL REFERENCES automations(id) ON DELETE CASCADE,
    status        TEXT    NOT NULL,            -- running | success | failed
    output        TEXT,
    error         TEXT,
    started_at    TEXT    NOT NULL,
    finished_at   TEXT
);

CREATE INDEX IF NOT EXISTS idx_runs_automation ON automation_runs(automation_id, id);

-- Log lines emitted by scripts via log(), print() and notify().
CREATE TABLE IF NOT EXISTS logs (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    automation_id INTEGER NOT NULL REFERENCES automations(id) ON DELETE CASCADE,
    level         TEXT    NOT NULL,            -- info | notify | error
    message       TEXT    NOT NULL,
    created_at    TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_logs_automation ON logs(automation_id, id);
