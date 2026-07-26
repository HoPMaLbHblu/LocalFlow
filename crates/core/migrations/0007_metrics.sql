-- System monitoring: one row a minute, kept for 30 days. Never leaves this PC.
CREATE TABLE metrics (
    at      INTEGER NOT NULL,
    cpu     REAL    NOT NULL,
    memory  REAL    NOT NULL,
    disk    REAL    NOT NULL,
    battery REAL
);

CREATE INDEX metrics_at ON metrics (at);
