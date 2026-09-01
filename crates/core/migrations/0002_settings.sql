-- Simple key/value store for application settings (desktop app preferences etc.).
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
