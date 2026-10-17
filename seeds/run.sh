#!/usr/bin/env bash
# Load the comprehensive demo dataset into the OpenScholar database.
#
# Populates EVERY module with live-like data (students, staff, classes,
# timetable, grades, attendance, fees/payments/fines/discounts, admissions,
# library, transport, hostel, HR/payroll/leave, welfare, notices, events,
# lessons, activities, notifications, outbox, attachments, audit).
#
# Safe to re-run: it wipes the domain tables first (keeps the admin login and
# settings). All seeded user logins use the admin password.
#
# Usage:  DATABASE_URL=postgresql://user:pass@host:5432/db ./seeds/run.sh
set -euo pipefail
cd "$(dirname "$0")/.."

: "${DATABASE_URL:=postgresql://admin1:dev-local-only@127.0.0.1:5432/school}"
echo "Seeding $DATABASE_URL ..."
for f in seeds/01_structure.sql seeds/02_students.sql seeds/03_academic_fees_ops.sql seeds/04_hr_welfare_content.sql; do
  echo "  -> $f"
  psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q -f "$f"
done
echo "Done. Seeded user logins use the admin password."
