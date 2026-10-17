CREATE TABLE IF NOT EXISTS applicants (
  id BIGSERIAL PRIMARY KEY, first_name TEXT NOT NULL, last_name TEXT NOT NULL,
  date_of_birth DATE NULL, gender TEXT NULL CHECK (gender IN ('male','female','other')),
  guardian_name TEXT NULL, guardian_phone TEXT NULL, guardian_email TEXT NULL,
  address TEXT NULL, applying_for TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'new' CHECK (status IN ('new','reviewing','accepted','rejected','enrolled')),
  notes TEXT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now());
CREATE INDEX IF NOT EXISTS idx_applicants_status ON applicants (status);
