-- Hardening constraints & indexes discovered during the ship-readiness audit.

-- Prevent duplicate payroll runs for the same staff member + period (double pay).
ALTER TABLE payroll_runs
    ADD CONSTRAINT payroll_runs_staff_period_key UNIQUE (staff_id, period_label);

-- Case-insensitive uniqueness for user emails (login matches lower(email), but
-- the column's UNIQUE was case-sensitive, allowing shadow accounts).
CREATE UNIQUE INDEX IF NOT EXISTS users_email_lower_key ON users (lower(email));

-- Indexes for the overdue / analytics scans that previously had none.
CREATE INDEX IF NOT EXISTS invoices_due_date_idx ON invoices (due_date);
CREATE INDEX IF NOT EXISTS payments_paid_at_idx ON payments (paid_at);
CREATE INDEX IF NOT EXISTS fee_fines_invoice_id_idx ON fee_fines (invoice_id);
