BEGIN IMMEDIATE;

CREATE TABLE IF NOT EXISTS plugin_installations (
    plugin_id TEXT PRIMARY KEY NOT NULL,
    version TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('builtin', 'external')),
    root_path TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    installed_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS permission_grants (
    plugin_id TEXT NOT NULL,
    capability TEXT NOT NULL,
    resource TEXT NOT NULL,
    granted_at TEXT NOT NULL,
    revoked_at TEXT,
    PRIMARY KEY (plugin_id, capability, resource),
    FOREIGN KEY (plugin_id) REFERENCES plugin_installations(plugin_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'canceled')),
    progress REAL NOT NULL DEFAULT 0 CHECK (progress >= 0 AND progress <= 1),
    cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK (cancel_requested IN (0, 1)),
    error_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS job_steps (
    job_id TEXT NOT NULL,
    step_index INTEGER NOT NULL,
    label TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'canceled')),
    detail TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (job_id, step_index),
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
);

PRAGMA user_version = 1;
COMMIT;
