# Changelog

All notable changes to OpenScholar are documented here.

## [0.1.0] — 2026-10-17

### Initial Release

**Core Modules**
- Student management — directory, profiles, enrollment, search (pg_trgm fuzzy + prefix fallback)
- Staff management — directory, profiles, departments
- Class management — creation, enrollment, school year association
- Subject management — CRUD with department grouping

**Academics**
- Timetable — period/room grid with slot assignment, teacher timetable view
- Grade book — assessments, score entry, weighted percentages, grade scale mapping
- Attendance — daily mark for class rosters with status tracking

**Financial**
- Fee categories and invoice generation
- Payment recording with receipt printing
- Fine/penalty tracking (folded into student balances)
- Discount management (percentage-based, student/category scoped)
- Outstanding balance dashboard

**Admissions**
- Applicant pipeline with status stages
- Kanban-style pipeline view
- Direct enrollment from accepted applicants

**Operations**
- Library — book catalog, loan checkout/return, overdue tracking
- Transport — route/stop management, rider assignment, capacity enforcement
- Hostel — room/bed management, student allocation
- Welfare — medical records, behavior incidents per student

**HR**
- Staff attendance tracking
- Leave request and approval workflow
- Payroll run management

**Communications**
- Notice board / bulletin with events calendar
- In-app notification system
- Email outbox

**Portal**
- Family portal — parent/student self-service for attendance, grades, fees, transport

**Print**
- Fee receipts
- Student report cards
- Academic transcripts

**Analytics**
- Dashboard with student, staff, class, and fee statistics
- Attendance rate and grade distribution
- At-risk student identification

**Security**
- Argon2 password hashing
- Session-based auth with configurable cookie security
- CSRF protection (Origin check + double-submit token)
- Per-account login lockout (5 failures → 15-min lock)
- Per-IP rate limiting (sliding window)
- Security headers (CSP, HSTS, X-Frame-Options, nosniff)
- Role-based access control (admin, teacher, accountant, parent, student)
- API bearer token authentication (fail-closed)
- Audit logging

**Infrastructure**
- Embedded SQL migrations (auto-apply on startup)
- Graceful shutdown (SIGTERM/SIGINT)
- Session sweeper (hourly cleanup)
- Docker multi-stage build
- docker-compose configuration
- Comprehensive seed data for all modules

**UI/UX**
- Modern minimal design system
- Dark mode (automatic)
- Responsive layout (mobile, tablet, desktop)
- Pagination on list views
- Breadcrumb navigation
- htmx-powered interactions
