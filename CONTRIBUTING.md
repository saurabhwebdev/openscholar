# Contributing to OpenScholar

Thank you for your interest in contributing to OpenScholar! Every contribution helps make school management better for everyone.

## How to Contribute

### Reporting Bugs

Found a bug? [Open an issue](https://github.com/saurabhwebdev/openscholar/issues/new) with:

1. **What happened** — describe the actual behavior
2. **What you expected** — describe what should have happened
3. **Steps to reproduce** — minimal steps to trigger the bug
4. **Environment** — Rust version, OS, PostgreSQL version, browser

### Suggesting Features

Have an idea? [Open a feature request](https://github.com/saurabhwebdev/openscholar/issues/new) describing:

1. **The problem** — what workflow is painful or missing?
2. **Proposed solution** — how should it work?
3. **Alternatives considered** — what else did you think of?

### Submitting Code

1. Fork the repository
2. Create a feature branch: `git checkout -b feat/my-feature`
3. Make your changes
4. Run tests: `cargo test`
5. Run the formatter: `cargo fmt`
6. Run the linter: `cargo clippy`
7. Commit with a descriptive message (see commit format below)
8. Push and open a pull request

## Development Setup

### Prerequisites

- **Rust** 1.75+ — [rustup.rs](https://rustup.rs/)
- **PostgreSQL** 14+ — [postgresql.org](https://www.postgresql.org/download/)
- **psql** CLI (included with PostgreSQL)

### Getting Started

```bash
# Clone your fork
git clone https://github.com/YOUR_USERNAME/openscholar.git
cd openscholar

# Create the development database
createdb school

# Configure environment
cp .env.example .env
# Edit .env — at minimum set DATABASE_URL

# Run in development mode
COOKIE_SECURE=false cargo run

# Load demo data (optional but recommended)
./seeds/run.sh
```

### Running Tests

```bash
# All tests
cargo test

# Only unit tests (no DB required)
cargo test --lib

# A specific test
cargo test test_name
```

### Code Quality

```bash
# Format code
cargo fmt

# Lint
cargo clippy -- -W clippy::all

# Check compilation without building
cargo check
```

## Code Style

- Follow standard Rust conventions and `rustfmt` defaults
- No comments unless the *why* is non-obvious — well-named code is self-documenting
- Prefer `match` over chains of `if let` when there are multiple branches
- Keep functions short and focused
- Use `AppError` for error propagation — don't unwrap in handlers

## Commit Message Format

We use conventional commits:

```
type(scope): short description

Optional longer description.
```

**Types:** `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `security`

**Scopes:** `students`, `fees`, `grades`, `auth`, `ui`, `api`, `db`, etc.

**Examples:**
```
feat(fees): add bulk invoice generation
fix(auth): correct lockout timer calculation
docs: update API endpoint table in README
refactor(models): extract pagination into shared module
```

## Architecture Overview

```
Request → Router → Middleware → Handler → Model → Database
                                  ↓
                               Template → HTML Response
```

### Key Layers

| Layer | Location | Purpose |
|---|---|---|
| **Routes** | `src/routes/` | HTTP handlers — parse input, call models, render templates |
| **Models** | `src/models/` | Database queries and business logic |
| **Templates** | `templates/` | Askama HTML templates (compiled into the binary) |
| **Auth** | `src/auth/` | Authentication, sessions, CSRF, rate limiting |
| **Middleware** | `src/auth/middleware.rs` | Security headers, auth gates, CSRF checks |

### Adding a New Module

1. **Model** — create `src/models/your_module.rs` with query functions
2. **Route** — create `src/routes/your_module.rs` with handlers
3. **Templates** — create `templates/your_module/*.html` extending `base.html`
4. **Register** — add `pub mod your_module;` in `src/models/mod.rs` and `src/routes/mod.rs`
5. **Wire up** — merge the router in `src/routes/mod.rs` → `app_router()`

### Database Migrations

Migrations live in `migrations/` and are applied automatically on startup via `sqlx::migrate!()`.

To add a new migration:

```bash
# Create a new migration file
# Use the next sequential number: NNNN_description.sql
touch migrations/0021_your_feature.sql
```

**Rules:**
- Use `IF NOT EXISTS` / `IF EXISTS` for idempotency
- Never modify an already-applied migration — create a new one
- Test with a fresh database: `dropdb school && createdb school && cargo run`
- Migration filenames must start with a number (e.g., `0021_`)

### Template Conventions

- Extend `base.html` for all authenticated pages
- Use the design system classes: `card`, `table-wrap`, `btn-primary`, `form-grid`, etc.
- Provide `section`, `section_label`, `today`, `user_name`, `user_initial` to the base layout
- Use `{% include "partials/pagination.html" %}` for paginated lists

## Pull Request Checklist

Before submitting a PR, verify:

- [ ] `cargo test` passes
- [ ] `cargo fmt` produces no changes
- [ ] `cargo clippy` has no warnings
- [ ] New features include tests where practical
- [ ] Templates use the existing design system classes
- [ ] Database changes are in a new migration file
- [ ] The PR description explains *what* and *why*

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code.

## Questions?

Open a [discussion](https://github.com/saurabhwebdev/openscholar/issues) or reach out in an issue. We're happy to help you get started.
