-- Deleted automations go to the trash first and can be restored.
ALTER TABLE automations ADD COLUMN deleted_at TEXT;

-- Every save keeps the previous version, so edits can be undone.
CREATE TABLE IF NOT EXISTS automation_versions (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    automation_id  INTEGER NOT NULL REFERENCES automations(id) ON DELETE CASCADE,
    name           TEXT    NOT NULL,
    description    TEXT    NOT NULL,
    lua_code       TEXT    NOT NULL,
    schedule       TEXT,
    run_on_startup INTEGER NOT NULL,
    watch_path     TEXT,
    watch_pattern  TEXT,
    saved_at       TEXT    NOT NULL   -- when this version was replaced
);

CREATE INDEX IF NOT EXISTS idx_versions_automation ON automation_versions(automation_id, id);
