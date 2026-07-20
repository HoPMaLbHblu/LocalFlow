-- Powerful functions (commands, keystrokes, shutdown, ...) only work when this is on.
ALTER TABLE automations ADD COLUMN allow_system INTEGER NOT NULL DEFAULT 0;

-- More ways to start an automation, as JSON:
-- {"hotkey": "Ctrl+Alt+K", "app_start": "steam", "app_exit": "code", "idle_minutes": 10, "usb": true}
ALTER TABLE automations ADD COLUMN triggers TEXT;
ALTER TABLE automation_versions ADD COLUMN triggers TEXT;
