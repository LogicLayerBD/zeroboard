-- Instance-wide settings changed from the admin panel. A single row (id = 1),
-- created on the first change. A NULL column means "use the env default"
-- (registration_enabled falls back to REGISTRATION_ENABLED).
CREATE TABLE instance_settings (
  id                   INTEGER PRIMARY KEY CHECK (id = 1),
  registration_enabled INTEGER CHECK (registration_enabled IN (0, 1)),
  updated_at           INTEGER NOT NULL
);
