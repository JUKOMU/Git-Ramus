BEGIN IMMEDIATE;

CREATE TABLE provider_instances (
    id TEXT PRIMARY KEY NOT NULL,
    provider_kind TEXT NOT NULL CHECK (provider_kind IN ('github','gitlab')),
    display_name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    api_base_url TEXT NOT NULL,
    custom_ca_path TEXT,
    last_validated_at TEXT,
    server_version TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(provider_kind, base_url)
);

CREATE TABLE provider_accounts (
    id TEXT PRIMARY KEY NOT NULL,
    instance_id TEXT NOT NULL,
    provider_user_id TEXT NOT NULL,
    username TEXT NOT NULL,
    display_name TEXT,
    avatar_url TEXT,
    secret_ref TEXT NOT NULL UNIQUE,
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0,1)),
    last_validated_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(instance_id, provider_user_id),
    UNIQUE(id, instance_id),
    FOREIGN KEY(instance_id) REFERENCES provider_instances(id) ON DELETE RESTRICT
);
CREATE UNIQUE INDEX idx_provider_accounts_one_default
    ON provider_accounts(instance_id) WHERE is_default = 1;
CREATE INDEX idx_provider_accounts_instance
    ON provider_accounts(instance_id, username, id);

CREATE TABLE provider_repository_bindings (
    repository_id TEXT NOT NULL,
    remote_name TEXT NOT NULL,
    provider_instance_id TEXT NOT NULL,
    provider_account_id TEXT,
    provider_repository_id TEXT NOT NULL,
    full_name TEXT NOT NULL,
    web_url TEXT NOT NULL,
    matched_url TEXT NOT NULL,
    binding_source TEXT NOT NULL CHECK (binding_source IN ('auto','manual')),
    bound_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY(repository_id, remote_name),
    FOREIGN KEY(repository_id, remote_name)
      REFERENCES repository_remotes(repository_id, name) ON DELETE CASCADE,
    FOREIGN KEY(provider_instance_id)
      REFERENCES provider_instances(id) ON DELETE RESTRICT,
    FOREIGN KEY(provider_account_id, provider_instance_id)
      REFERENCES provider_accounts(id, instance_id) ON DELETE RESTRICT
);
CREATE INDEX idx_provider_bindings_instance
    ON provider_repository_bindings(provider_instance_id, repository_id, remote_name);
CREATE INDEX idx_provider_bindings_account
    ON provider_repository_bindings(provider_account_id, repository_id, remote_name);

CREATE TABLE provider_secret_cleanup (
    secret_ref TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL,
    last_attempt_at TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    last_error_code TEXT
);

PRAGMA user_version = 3;
COMMIT;
