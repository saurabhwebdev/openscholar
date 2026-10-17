# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in OpenScholar, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please email: **saurabh.thakur@unisonmining.com**

Include:

1. **Description** of the vulnerability
2. **Steps to reproduce** the issue
3. **Impact assessment** — what an attacker could achieve
4. **Suggested fix** (if you have one)

## Response Timeline

- **Acknowledgement**: Within 48 hours
- **Initial assessment**: Within 1 week
- **Fix and release**: As soon as practical, typically within 2 weeks for critical issues

## Scope

The following are in scope:

- Authentication and session management
- Authorization and access control bypass
- Cross-site scripting (XSS)
- Cross-site request forgery (CSRF)
- SQL injection
- Server-side request forgery (SSRF)
- Information disclosure (PII exposure)
- Denial of service (application-level)
- Insecure defaults or configurations

## Security Features

OpenScholar includes several security measures:

- **Argon2** password hashing with random salts
- **CSRF protection** — Origin/Referer validation + double-submit cookie tokens
- **Rate limiting** — per-account lockout (DB-backed) + per-IP throttling (in-memory)
- **Security headers** — CSP, HSTS, X-Frame-Options DENY, X-Content-Type-Options nosniff
- **Session management** — HttpOnly cookies, configurable Secure flag, 30-day TTL, server-side invalidation
- **Input validation** — server-side validation on all forms
- **Role-based access** — middleware-enforced role gates
- **API auth** — bearer token with fail-closed default (disabled unless explicitly configured)
- **Audit logging** — all create/update/delete actions logged
- **Template escaping** — Askama auto-escapes all rendered values (XSS defense)

## Supported Versions

| Version | Supported |
|---|---|
| 0.1.x | Yes |

## Recognition

We appreciate security researchers who help keep OpenScholar safe. With your permission, we'll credit you in the release notes for any vulnerability you report.
