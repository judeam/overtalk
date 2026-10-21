-- Widen the provider constraint without mutating an already-applied migration.
-- Copy dependent drafts first; dropping the old parent with foreign keys enabled
-- would otherwise cascade-delete them. SQLx executes this migration atomically.
CREATE TABLE workspace_connections_next (
 id TEXT PRIMARY KEY,
 provider TEXT NOT NULL CHECK(provider IN (
  'linear','jira','google','confluence','linear_mcp','atlassian_mcp',
  'notion_mcp','microsoft','slack','github'
 )),
 name TEXT NOT NULL, base_url TEXT NOT NULL DEFAULT '', username TEXT NOT NULL DEFAULT '',
 project_id TEXT REFERENCES projects(id) ON DELETE SET NULL, created_at TEXT NOT NULL
);
INSERT INTO workspace_connections_next
 SELECT id,provider,name,base_url,username,project_id,created_at FROM workspace_connections;

CREATE TABLE outgoing_drafts_next (
 id TEXT PRIMARY KEY,
 connection_id TEXT NOT NULL REFERENCES workspace_connections_next(id) ON DELETE CASCADE,
 kind TEXT NOT NULL, target TEXT NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL,
 expected_revision TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'draft',
 created_at TEXT NOT NULL, result TEXT
);
INSERT INTO outgoing_drafts_next
 SELECT id,connection_id,kind,target,title,body,expected_revision,status,created_at,result
 FROM outgoing_drafts;

DROP TABLE outgoing_drafts;
DROP TABLE workspace_connections;
ALTER TABLE workspace_connections_next RENAME TO workspace_connections;
ALTER TABLE outgoing_drafts_next RENAME TO outgoing_drafts;
