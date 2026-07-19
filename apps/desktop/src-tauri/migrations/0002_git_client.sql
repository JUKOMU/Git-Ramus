BEGIN IMMEDIATE;

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY NOT NULL,
    root_path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    scan_depth INTEGER NOT NULL DEFAULT 3 CHECK (scan_depth >= 0),
    exclude_patterns_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS workspace_projects (
    workspace_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0 CHECK (position >= 0),
    PRIMARY KEY (workspace_id, project_id),
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_workspace_projects_project ON workspace_projects(project_id);

CREATE TABLE IF NOT EXISTS repositories (
    id TEXT PRIMARY KEY NOT NULL,
    canonical_path TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('normal', 'bare', 'worktree')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS project_repositories (
    project_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    relative_path TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (project_id, repository_id),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (repository_id) REFERENCES repositories(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_project_repositories_repository ON project_repositories(repository_id);

CREATE TABLE IF NOT EXISTS repository_snapshots (
    id TEXT PRIMARY KEY NOT NULL,
    repository_id TEXT NOT NULL,
    captured_at TEXT NOT NULL,
    head_oid TEXT,
    branch TEXT,
    upstream TEXT,
    ahead INTEGER NOT NULL DEFAULT 0,
    behind INTEGER NOT NULL DEFAULT 0,
    dirty INTEGER NOT NULL DEFAULT 0 CHECK (dirty IN (0,1)),
    staged_count INTEGER NOT NULL DEFAULT 0,
    unstaged_count INTEGER NOT NULL DEFAULT 0,
    untracked_count INTEGER NOT NULL DEFAULT 0,
    conflicted_count INTEGER NOT NULL DEFAULT 0,
    refresh_error_summary TEXT,
    FOREIGN KEY (repository_id) REFERENCES repositories(id) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS idx_repository_snapshots_repository ON repository_snapshots(repository_id, captured_at DESC);
CREATE TABLE IF NOT EXISTS repository_remotes (
    repository_id TEXT NOT NULL,
    name TEXT NOT NULL,
    fetch_url TEXT,
    push_url TEXT,
    PRIMARY KEY (repository_id, name),
    FOREIGN KEY (repository_id) REFERENCES repositories(id) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS trusted_repositories (
    repository_id TEXT PRIMARY KEY NOT NULL,
    trusted_at TEXT NOT NULL,
    trust_version INTEGER NOT NULL DEFAULT 1 CHECK (trust_version > 0),
    FOREIGN KEY (repository_id) REFERENCES repositories(id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS identity_profiles (
    id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL,
    user_name TEXT NOT NULL,
    user_email TEXT NOT NULL,
    gpg_format TEXT,
    signing_key TEXT,
    sign_commits INTEGER NOT NULL DEFAULT 0 CHECK (sign_commits IN (0,1)),
    sign_tags INTEGER NOT NULL DEFAULT 0 CHECK (sign_tags IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS repository_identity_bindings (
    repository_id TEXT PRIMARY KEY NOT NULL,
    identity_profile_id TEXT NOT NULL,
    managed INTEGER NOT NULL DEFAULT 1 CHECK (managed IN (0,1)),
    bound_at TEXT NOT NULL,
    FOREIGN KEY (repository_id) REFERENCES repositories(id) ON DELETE RESTRICT,
    FOREIGN KEY (identity_profile_id) REFERENCES identity_profiles(id) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS themes (
    theme_id TEXT PRIMARY KEY NOT NULL,
    plugin_id TEXT NOT NULL,
    version TEXT NOT NULL,
    definition_json TEXT NOT NULL,
    is_valid INTEGER NOT NULL DEFAULT 1 CHECK (is_valid IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS global_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    global_identity_profile_id TEXT,
    active_theme_id TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (global_identity_profile_id) REFERENCES identity_profiles(id) ON DELETE SET NULL,
    FOREIGN KEY (active_theme_id) REFERENCES themes(theme_id) ON DELETE SET NULL
);
INSERT OR IGNORE INTO global_settings (id) VALUES (1);

PRAGMA user_version = 2;
COMMIT;
