-- Values scripts save with store.set(), kept between runs.
CREATE TABLE IF NOT EXISTS automation_store (
    automation_id INTEGER NOT NULL REFERENCES automations(id) ON DELETE CASCADE,
    key           TEXT    NOT NULL,
    value         TEXT    NOT NULL,  -- JSON
    PRIMARY KEY (automation_id, key)
);
