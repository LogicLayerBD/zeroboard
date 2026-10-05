-- Instance-wide role. Workspace permissions live in workspace_members.role;
-- this role gates instance administration (first registered user becomes admin).
ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'member' CHECK(role IN ('admin','member'));
