-- Round-2 hardening: constraints, indexes, rate-limit table, search acceleration.

-- A3: room can't be double-booked in a period (backstop for the TOCTOU check).
CREATE UNIQUE INDEX IF NOT EXISTS slots_room_period_uniq
    ON slots (period_id, lower(btrim(room)))
    WHERE room IS NOT NULL AND btrim(room) <> '';

-- A4: drop the redundant non-unique payroll index now that a UNIQUE exists.
DROP INDEX IF EXISTS idx_payroll_staff;

-- D9: at most one current school year / semester at a time.
CREATE UNIQUE INDEX IF NOT EXISTS school_years_single_current
    ON school_years ((is_current)) WHERE is_current;
CREATE UNIQUE INDEX IF NOT EXISTS semesters_single_current
    ON semesters ((is_current)) WHERE is_current;

-- D2: one login maps to at most one student / staff row.
CREATE UNIQUE INDEX IF NOT EXISTS students_user_id_uniq
    ON students (user_id) WHERE user_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS staff_user_id_uniq
    ON staff (user_id) WHERE user_id IS NOT NULL;

-- B1: login attempt tracking for per-account lockout (survives restarts).
CREATE TABLE IF NOT EXISTS login_attempts (
    email_lower  TEXT PRIMARY KEY,
    fail_count   INTEGER NOT NULL DEFAULT 0,
    first_fail_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    locked_until TIMESTAMPTZ
);

-- B5: sort-key indexes for the list views that gained LIMITs.
CREATE INDEX IF NOT EXISTS notifications_user_created_idx ON notifications (user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS lessons_date_idx ON lessons (date DESC);
CREATE INDEX IF NOT EXISTS leave_requests_created_idx ON leave_requests (created_at DESC);
CREATE INDEX IF NOT EXISTS payroll_runs_created_idx ON payroll_runs (created_at DESC);

-- C11: accelerate substring (ILIKE '%x%') search. Best-effort: if the DB role
-- can't create the extension, skip the trgm indexes (search still works, just
-- seq-scans) rather than failing the whole migration/boot.
DO $$
BEGIN
    CREATE EXTENSION IF NOT EXISTS pg_trgm;
    CREATE INDEX IF NOT EXISTS students_name_trgm ON students
        USING gin ((lower(first_name || ' ' || last_name || ' ' || admission_no)) gin_trgm_ops);
    CREATE INDEX IF NOT EXISTS staff_name_trgm ON staff
        USING gin ((lower(first_name || ' ' || last_name || ' ' || employee_no)) gin_trgm_ops);
    CREATE INDEX IF NOT EXISTS invoices_title_trgm ON invoices
        USING gin ((lower(title)) gin_trgm_ops);
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'pg_trgm unavailable (%); substring search will seq-scan', SQLERRM;
END $$;
