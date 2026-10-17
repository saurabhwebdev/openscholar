-- Hardening constraints & indexes discovered during the ship-readiness audit.

-- De-duplicate any pre-existing double-pay rows (keep the lowest id) BEFORE
-- enforcing uniqueness, so this migration is safe to apply to a database that
-- already contains duplicate payroll runs instead of aborting startup.
DELETE FROM payroll_runs a USING payroll_runs b
    WHERE a.staff_id = b.staff_id AND a.period_label = b.period_label AND a.id > b.id;

-- Prevent duplicate payroll runs for the same staff member + period (double pay).
-- Index-backed (a bare ADD CONSTRAINT has no IF NOT EXISTS form, so it is not
-- idempotent); a unique index enforces the same thing and the 23505 handling in
-- create_payroll works identically.
ALTER TABLE payroll_runs DROP CONSTRAINT IF EXISTS payroll_runs_staff_period_key;
CREATE UNIQUE INDEX IF NOT EXISTS payroll_runs_staff_period_key
    ON payroll_runs (staff_id, period_label);

-- Case-insensitive uniqueness for user emails (login matches lower(email), but
-- the column's UNIQUE was case-sensitive, allowing shadow accounts).
CREATE UNIQUE INDEX IF NOT EXISTS users_email_lower_key ON users (lower(email));

-- Indexes for the overdue / analytics scans that previously had none.
CREATE INDEX IF NOT EXISTS invoices_due_date_idx ON invoices (due_date);
CREATE INDEX IF NOT EXISTS payments_paid_at_idx ON payments (paid_at);
CREATE INDEX IF NOT EXISTS fee_fines_invoice_id_idx ON fee_fines (invoice_id);
