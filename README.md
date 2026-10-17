<div align="center">

# 🎓 OpenScholar

**Modern, open-source school management system built with Rust**

[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange)](https://www.rust-lang.org/)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen)](CONTRIBUTING.md)
[![GitHub Stars](https://img.shields.io/github/stars/saurabhwebdev/openscholar)](https://github.com/saurabhwebdev/openscholar)

*Everything a school needs — students, staff, classes, fees, grades, timetable, library, transport, hostel, HR, admissions, analytics — in one fast, secure application.*

</div>

---

## What is OpenScholar?

OpenScholar is a comprehensive school management system that handles every aspect of school operations — from student enrollment and fee collection to timetable scheduling and HR payroll. Built with Rust and Axum for performance and reliability, it serves a modern web UI with server-rendered templates, real-time interactions via htmx, and a REST API for integrations. It's designed to be self-hosted, easy to deploy, and simple to extend.

## Features

### 👩‍🎓 Student Management
- Student directory with full profiles, search (pg_trgm fuzzy), and pagination
- Enrollment tracking across school years and classes
- Guardian/parent linking with family portal access
- Status management (active, inactive, graduated, transferred)

### 👨‍💼 Staff & HR
- Staff directory with profiles, departments, and qualifications
- Daily attendance tracking for staff
- Leave request and approval workflow
- Payroll run management with period tracking

### 📚 Academics
- Class creation with school year association
- Subject management with department grouping
- Timetable grid — drag-and-drop slot assignment per room and period
- Teacher timetable view
- Grade book with weighted assessments, score entry, and grade scale mapping

### 💰 Financial
- Fee categories and invoice generation
- Payment recording with receipt printing
- Fine/penalty tracking folded into balances
- Discount management (percentage-based, scoped to student/category)
- Outstanding balance dashboard with top debtors

### 📋 Admissions
- Applicant pipeline with status stages (inquiry → applied → accepted → enrolled)
- Kanban-style pipeline view
- Direct enrollment from accepted applicants

### 📖 Library
- Book catalog with search
- Loan checkout and return tracking
- Overdue loan visibility

### 🚌 Transport
- Route management with stops and capacity
- Rider assignment per route
- Student transport view

### 🏠 Hostel
- Room and bed management
- Student allocation and release
- Occupancy tracking

### 🏥 Welfare
- Medical records per student
- Behavior incident tracking (minor, major, positive)
- Summary statistics on student detail pages

### 🎯 Activities & Lessons
- Extracurricular activities (clubs, sports, events, service)
- Membership tracking
- Lesson plan management with materials

### 📢 Communications
- Notice board / bulletin with events
- In-app notification system with read/unread tracking
- Email outbox for sent communications

### 📊 Analytics Dashboard
- Student, staff, class, and fee summary statistics
- Attendance rates and grade distribution
- At-risk student identification
- Fee collection overview

### 👨‍👩‍👧 Family Portal
- Parent and student self-service portal
- View attendance, grades, fees, and transport info
- Linked children management

### 🔒 Security
- Argon2 password hashing
- Session-based authentication with 30-day TTL
- CSRF protection (Origin/Referer validation + double-submit token)
- Per-account login lockout (5 failures → 15-min lock)
- Per-IP rate limiting (in-memory sliding window)
- Content Security Policy, HSTS, X-Frame-Options headers
- Role-based access control (admin, teacher, accountant, parent, student)

### 🖨️ Print
- Fee receipts
- Student report cards
- Academic transcripts

### 🔍 Audit & Admin
- Full audit log of create/update/delete actions
- User management with role assignment
- School settings (year, semester, grade scale)
- File attachment system (role-gated downloads)

### 🌐 API
- REST API at `/api/v1/*` with bearer token authentication
- Health check endpoint
- JSON responses for integrations

### 🎨 Design
- Modern, minimal UI with Inter font
- Dark mode support (automatic via `prefers-color-scheme`)
- Fully responsive — works on desktop, tablet, and mobile
- Pagination on all list views

## Tech Stack

| Layer | Technology |
|---|---|
| **Backend** | Rust, Axum 0.8, Tokio |
| **Templates** | Askama 0.16 (compiled, type-safe) |
| **Database** | PostgreSQL 14+, SQLx 0.9 (runtime queries) |
| **Auth** | Argon2 password hashing, UUID sessions |
| **Frontend** | Tailwind CSS, htmx, Inter font |
| **Security** | CSRF tokens, rate limiting, CSP headers, HSTS |
| **Deployment** | Docker, docker-compose |

## Quick Start

### Prerequisites

- **Rust** 1.75 or later — [install](https://rustup.rs/)
- **PostgreSQL** 14 or later — [install](https://www.postgresql.org/download/)

### Setup

```bash
# Clone the repository
git clone https://github.com/saurabhwebdev/openscholar.git
cd openscholar

# Create the database
createdb school

# Configure environment
cp .env.example .env
# Edit .env with your database URL and settings

# Run (migrations execute automatically on startup)
cargo run
```

Visit **http://localhost:8081** and log in with:

| Role | Email | Password |
|---|---|---|
| Admin | `admin@example.com` | *(set via ADMIN_PASSWORD env var)* |

### Load Demo Data

To populate every module with realistic sample data (30 students, 10 staff, classes, grades, fees, library, transport, hostel, HR, welfare, notices, activities, and more):

```bash
./seeds/run.sh
```

Demo logins (all use the admin password):

| Role | Email |
|---|---|
| Admin | `admin@example.com` |
| Teacher | `m.hale@riverside.example` |
| Parent | `parent1@riverside.example` |
| Student | `student1@riverside.example` |

## Docker

```bash
# Build and start (uses host Postgres by default)
docker-compose up -d

# Or with a custom database URL
DATABASE_URL=postgresql://user:pass@host:5432/school docker-compose up -d
```

The Docker image is a multi-stage build: Rust compilation in a builder stage, then a minimal Debian slim runtime image (~50 MB).

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `DATABASE_URL` | *(required)* | PostgreSQL connection string |
| `BIND_ADDR` | `0.0.0.0:8081` | Server listen address |
| `ADMIN_EMAIL` | `admin@example.com` | Default admin account email |
| `ADMIN_PASSWORD` | `changeme-admin-123` | Default admin password (**change in production**) |
| `ADMIN_NAME` | `Administrator` | Default admin display name |
| `API_TOKEN` | *(disabled)* | Bearer token for REST API (disabled if unset or default) |
| `COOKIE_SECURE` | `true` | Set to `false` for local HTTP development |
| `RUST_LOG` | `opencodetest=info` | Log level filter |
| `TZ` | `UTC` | Server timezone |
| `DB_MAX_CONNECTIONS` | `10` | Database connection pool size |
| `S3_ENDPOINT` | — | S3-compatible storage endpoint (optional) |
| `S3_BUCKET` | — | S3 bucket name |
| `S3_REGION` | `us-east-1` | S3 region |
| `S3_ACCESS_KEY` | — | S3 access key |
| `S3_SECRET_KEY` | — | S3 secret key |
| `S3_PUBLIC_BASE_URL` | — | Public URL prefix for uploaded files |

## Project Structure

```
openscholar/
├── src/
│   ├── main.rs            # Entry point, server startup, graceful shutdown
│   ├── lib.rs             # Module declarations
│   ├── auth/              # Authentication, sessions, CSRF, rate limiting
│   │   ├── middleware.rs   # Auth middleware, security headers, CSRF protection
│   │   ├── routes.rs       # Login/logout handlers
│   │   ├── session.rs      # Session management, cookie handling
│   │   ├── password.rs     # Argon2 hashing
│   │   └── ratelimit.rs    # Per-IP rate limiter
│   ├── models/            # Database models and queries
│   │   ├── student.rs      # Student CRUD, search
│   │   ├── fees.rs         # Invoices, payments, balances
│   │   ├── grades.rs       # Assessments, scores, grade scales
│   │   └── ...             # One file per domain
│   ├── routes/            # HTTP route handlers
│   │   ├── mod.rs          # Router assembly, middleware stack
│   │   ├── students.rs     # Student pages
│   │   ├── fees.rs         # Fee pages
│   │   └── ...             # One file per module
│   ├── pagination.rs      # Reusable pagination logic
│   ├── config.rs          # Environment configuration
│   ├── db.rs              # Database pool setup
│   ├── state.rs           # Shared application state
│   ├── error.rs           # Error types and handling
│   └── util.rs            # Formatting helpers
├── templates/             # Askama HTML templates
│   ├── base.html           # Layout shell (sidebar, topbar)
│   ├── partials/           # Reusable template components
│   ├── students/           # Student pages
│   ├── fees/               # Fee pages
│   └── ...                 # One directory per module
├── static/
│   ├── css/               # Stylesheets (app.css, tailwind.css, print.css)
│   └── js/                # Client scripts (htmx)
├── migrations/            # SQL migrations (auto-applied on startup)
├── seeds/                 # Demo data scripts
├── Dockerfile             # Multi-stage Docker build
├── docker-compose.yml     # Compose configuration
└── Cargo.toml             # Rust dependencies
```

## API

OpenScholar exposes a REST API at `/api/v1/` for integrations. Authenticate with a bearer token:

```bash
# Set API_TOKEN in your environment first
curl -H "Authorization: Bearer YOUR_TOKEN" http://localhost:8081/api/v1/health
```

The API is disabled unless a non-default `API_TOKEN` is configured. The bearer path is restricted to `/api/` routes — it cannot access HTML pages that carry student PII.

**Available endpoints:**

| Method | Path | Description |
|---|---|---|
| GET | `/api/v1/health` | Health check (no auth required) |
| GET | `/api/v1/students` | List students |
| GET | `/api/v1/students/:id` | Student detail |
| GET | `/api/v1/staff` | List staff |
| GET | `/api/v1/classes` | List classes |

## Contributing

We welcome contributions of all kinds! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on:

- Reporting bugs and suggesting features
- Setting up a development environment
- Code style and conventions
- Submitting pull requests

## License

OpenScholar is released under the [MIT License](LICENSE). You are free to use, modify, and distribute it for any purpose.

---

<div align="center">

**Built with Rust 🦀 and love ❤️**

[Report a Bug](https://github.com/saurabhwebdev/openscholar/issues) · [Request a Feature](https://github.com/saurabhwebdev/openscholar/issues) · [Contribute](CONTRIBUTING.md)

</div>
