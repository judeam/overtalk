PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY, token_hash TEXT NOT NULL UNIQUE, cache_salt TEXT NOT NULL, customer TEXT UNIQUE,
    created_at INTEGER NOT NULL, revoked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS grants (
    source TEXT PRIMARY KEY, account TEXT NOT NULL REFERENCES accounts(id),
    cycle TEXT UNIQUE, payment TEXT NOT NULL, units INTEGER NOT NULL CHECK(units > 0),
    paid_cents INTEGER NOT NULL CHECK(paid_cents > 0),
    revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked >= 0),
    spent INTEGER NOT NULL DEFAULT 0 CHECK(spent >= 0),
    held INTEGER NOT NULL DEFAULT 0 CHECK(held >= 0),
    starts INTEGER NOT NULL, expires INTEGER
);
CREATE INDEX IF NOT EXISTS grants_account ON grants(account);
CREATE INDEX IF NOT EXISTS grants_payment ON grants(payment);
CREATE TABLE IF NOT EXISTS requests (
    id TEXT PRIMARY KEY, account TEXT NOT NULL REFERENCES accounts(id),
    client_id TEXT NOT NULL, status TEXT NOT NULL,
    reserved INTEGER NOT NULL, charged INTEGER, created_at INTEGER NOT NULL,
    UNIQUE(account, client_id)
);
CREATE TABLE IF NOT EXISTS allocations (
    request TEXT NOT NULL REFERENCES requests(id), grant_source TEXT NOT NULL REFERENCES grants(source),
    units INTEGER NOT NULL, PRIMARY KEY(request, grant_source)
);
CREATE TABLE IF NOT EXISTS disputes (payment TEXT PRIMARY KEY, blocked INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS checkouts (
    id TEXT PRIMARY KEY, account TEXT NOT NULL REFERENCES accounts(id), plan TEXT NOT NULL,
    idem TEXT NOT NULL, UNIQUE(account, idem)
);
CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY, state TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0, next_attempt INTEGER NOT NULL DEFAULT 0
);
-- Privatemode API keys the operator created in the Privatemode portal, each with
-- monthly token limits sized for the subscription allowance and an expiry date. A
-- subscriber's desktop runs Privatemode's proxy with its own key, so meeting
-- text never reaches this server. One live key per account.
CREATE TABLE IF NOT EXISTS summary_keys (
    id TEXT PRIMARY KEY, label TEXT NOT NULL, secret TEXT NOT NULL UNIQUE,
    expires INTEGER NOT NULL, account TEXT REFERENCES accounts(id),
    assigned INTEGER, retired INTEGER
);
CREATE UNIQUE INDEX IF NOT EXISTS summary_keys_live ON summary_keys(account)
    WHERE account IS NOT NULL AND retired IS NULL;
