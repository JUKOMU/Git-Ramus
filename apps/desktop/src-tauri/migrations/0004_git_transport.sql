BEGIN IMMEDIATE;

CREATE TABLE transport_profiles (
    id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('ssh','https')),
    ssh_key_path TEXT,
    ssh_variant TEXT,
    ssh_identities_only INTEGER CHECK (ssh_identities_only IN (0,1)),
    https_username TEXT,
    https_use_http_path INTEGER CHECK (https_use_http_path IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (
        (
            kind = 'ssh'
            AND ssh_key_path IS NOT NULL
            AND length(trim(ssh_key_path)) > 0
            AND ssh_variant = 'ssh'
            AND ssh_identities_only IS NOT NULL
            AND https_username IS NULL
            AND https_use_http_path IS NULL
        )
        OR
        (
            kind = 'https'
            AND ssh_key_path IS NULL
            AND ssh_variant IS NULL
            AND ssh_identities_only IS NULL
            AND https_username IS NOT NULL
            AND length(trim(https_username)) > 0
            AND https_use_http_path = 1
        )
    )
);

CREATE TABLE repository_transport_bindings (
    repository_id TEXT PRIMARY KEY NOT NULL,
    transport_profile_id TEXT NOT NULL,
    before_config_json TEXT NOT NULL,
    applied_config_json TEXT NOT NULL,
    applied_config_hash TEXT NOT NULL,
    drift_status TEXT NOT NULL DEFAULT 'clean' CHECK (drift_status IN ('clean','drifted')),
    bound_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(repository_id) REFERENCES repositories(id) ON DELETE RESTRICT,
    FOREIGN KEY(transport_profile_id) REFERENCES transport_profiles(id) ON DELETE RESTRICT
);

CREATE TABLE transport_config_repairs (
    id TEXT PRIMARY KEY NOT NULL,
    repository_id TEXT NOT NULL,
    before_config_json TEXT NOT NULL,
    attempted_config_json TEXT NOT NULL,
    error_code TEXT NOT NULL,
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    FOREIGN KEY(repository_id) REFERENCES repositories(id) ON DELETE RESTRICT
);

CREATE TABLE git_clone_operations (
    operation_id TEXT PRIMARY KEY NOT NULL,
    job_id TEXT NOT NULL UNIQUE,
    source_summary TEXT NOT NULL,
    intent_id TEXT,
    transport_profile_id TEXT,
    provider_instance_id TEXT,
    provider_account_id TEXT,
    provider_repository_id TEXT,
    staging_path TEXT NOT NULL,
    owner_marker_path TEXT NOT NULL,
    final_path TEXT NOT NULL,
    project_target_json TEXT NOT NULL,
    current_stage TEXT NOT NULL CHECK (
        current_stage IN (
            'validating',
            'awaitingAuthentication',
            'transferring',
            'checkingOut',
            'applyingProfile',
            'registering',
            'refreshing',
            'completed',
            'failed',
            'cancelled',
            'partial'
        )
    ),
    filesystem_complete INTEGER NOT NULL DEFAULT 0 CHECK (filesystem_complete IN (0,1)),
    repository_id TEXT,
    project_id TEXT,
    profile_applied INTEGER NOT NULL DEFAULT 0 CHECK (profile_applied IN (0,1)),
    provider_binding_complete INTEGER NOT NULL DEFAULT 0 CHECK (provider_binding_complete IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(job_id) REFERENCES jobs(id) ON DELETE CASCADE,
    FOREIGN KEY(repository_id) REFERENCES repositories(id) ON DELETE RESTRICT,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE RESTRICT
);

CREATE INDEX idx_transport_bindings_profile
    ON repository_transport_bindings(transport_profile_id, repository_id);
CREATE INDEX idx_transport_repairs_repository
    ON transport_config_repairs(repository_id, resolved_at);

PRAGMA user_version = 4;
COMMIT;
