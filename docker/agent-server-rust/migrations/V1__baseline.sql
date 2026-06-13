-- Baseline schema: sessions, wechat_keys, sync_state, context
-- Uses IF NOT EXISTS for idempotency (safe on existing DBs)

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    linux_user TEXT NOT NULL UNIQUE,
    display TEXT NOT NULL UNIQUE,
    dbus_address TEXT,
    vnc_port INTEGER UNIQUE,
    status TEXT NOT NULL DEFAULT 'stopped',
    login_state TEXT NOT NULL DEFAULT 'logged_out',
    logged_in_user TEXT,
    wechat_pid INTEGER,
    xvfb_pid INTEGER,
    dbus_pid INTEGER,
    error_message TEXT,
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_name ON sessions(name);

CREATE TABLE IF NOT EXISTS wechat_keys (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    account_dir TEXT NOT NULL,
    db_name TEXT NOT NULL,
    hex_key TEXT NOT NULL,
    verified_at TEXT,
    UNIQUE(session_id, account_dir, db_name)
);

CREATE INDEX IF NOT EXISTS idx_wechat_keys_session_account
    ON wechat_keys(session_id, account_dir);

CREATE TABLE IF NOT EXISTS sync_state (
    session_id TEXT REFERENCES sessions(id),
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT DEFAULT (datetime('now')),
    PRIMARY KEY (session_id, key)
);

CREATE TABLE IF NOT EXISTS context (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id),
    app_state TEXT NOT NULL,
    updated_at TEXT DEFAULT (datetime('now'))
);
