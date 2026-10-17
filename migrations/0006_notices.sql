-- Fedena parity: bulletin (notices) + calendar (events).

CREATE TABLE IF NOT EXISTS notices (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL, body TEXT NOT NULL, audience TEXT NOT NULL DEFAULT 'all' CHECK (audience IN ('all','staff','parents')), published_at TIMESTAMPTZ NOT NULL DEFAULT now(), expires_at TIMESTAMPTZ NULL, created_by BIGINT NULL REFERENCES users(id) ON DELETE SET NULL);
CREATE TABLE IF NOT EXISTS events (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL, description TEXT NULL, starts_at TIMESTAMPTZ NOT NULL, ends_at TIMESTAMPTZ NULL, location TEXT NULL, created_by BIGINT NULL REFERENCES users(id) ON DELETE SET NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now());
CREATE INDEX IF NOT EXISTS idx_notices_audience ON notices (audience, published_at);
CREATE INDEX IF NOT EXISTS idx_events_starts ON events (starts_at);
