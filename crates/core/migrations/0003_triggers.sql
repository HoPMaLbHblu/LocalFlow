-- More ways to start an automation besides a schedule.
ALTER TABLE automations ADD COLUMN run_on_startup INTEGER NOT NULL DEFAULT 0; -- run when LocalFlow starts
ALTER TABLE automations ADD COLUMN watch_path TEXT;                           -- run when a file appears here
ALTER TABLE automations ADD COLUMN watch_pattern TEXT;                        -- only files matching this, e.g. *.pdf
