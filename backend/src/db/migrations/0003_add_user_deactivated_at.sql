-- Instance admins can deactivate accounts instead of deleting them (users are
-- referenced by cards, comments, attachments, ...). NULL = active; otherwise the
-- Unix-ms time of deactivation. Deactivated users cannot sign in or use tokens.
ALTER TABLE users ADD COLUMN deactivated_at INTEGER;
