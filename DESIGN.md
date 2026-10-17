# OpenScholar — DESIGN.md

Product: **OpenScholar**, a single-school management suite (Rust port inspired by
Skuul/LavSMS data models, renamed and rebuilt). This file is the source of truth
for visual language so every page stays consistent.

## Principles (modern, minimal, spacious, refreshing)

1. One focal point per screen; size/weight/color create hierarchy, never more elements.
2. Whitespace is premium: section gaps 48–128px, card padding 24–32px, related items 8–12px apart.
3. Neutral-first: ~90% slate surfaces, one indigo accent reserved for action + emphasis.
4. 4/8 spacing scale only: 2, 4, 8, 12, 16, 24, 32, 48, 64, 96, 128. No off-grid values.
5. Type: Inter, 2 weights (400/600), 3–4 sizes per page; headings tracked tighter.
6. Every interactive element ships default / hover / active / focus-visible / disabled.
7. Empty, loading (skeleton), and error states are first-class, not afterthoughts.

## Tokens (`static/css/app.css`)

| Token | Light | Dark | Role |
|---|---|---|---|
| `--bg` | `#f6f7fb` (cool-tinted off-white) | `#0e0e13` | Page background |
| `--surface` | `#ffffff` | `#17171e` | Cards, panels |
| `--surface-raised` | `#ffffff` | `#1f1f29` | Elevated layers |
| `--text` | `#16161d` (near-black, not #000) | `#e7e7ec` | Body text |
| `--text-muted` | `#5b5f76` (readable, ≥4.5:1) | `#a6a8bd` | Secondary text |
| `--border` | `color-mix(in oklch, var(--text) 12%, transparent)` | same formula | Hairlines |
| `--accent` | `#4f46e5` (AA-safe on light) | `#a5b4fc` (AA-safe on dark) | Links, focus, text accents |
| `--accent-hover` | `#4338ca` | `#818cf8` | Hover |
| `--accent-solid` | `#4f46e5` (white text 6.3:1) | `#4f46e5` (white text 6.3:1) | Button fills only |
| `--accent-soft` | `accent 12% tint` | same formula | Icon tiles, avatar chips |
| `--ok / --warn / --err` | `#047857 / #b45309 / #b91c1c` | softened tints | Status, sparingly |
| `--radius` | `12px` cards | — | One radius scale |
| `--radius-sm` | `8px` buttons/inputs/pills-content | — | Linear compact spec |
| `--shadow-sm/md` | layered 2-stop shadows | elevation via lighter surface | Depth |

Type scale: 12 / 14 / 16 / 20 / 24 / 30. Body 16px @ 1.6; headings 1.1–1.2, `letter-spacing: -0.02em`.
Data/numbers: `font-variant-numeric: tabular-nums`.

## Component classes (in `app.css`, used by all Askama templates)

- `.btn-primary` (44px, accent, hover −12%, active nudge, focus ring, disabled state)
- `.btn-secondary` / `.btn-ghost` — one primary per view, secondary for the rest
- `.input` (44px, visible border, accent ring on focus, real `<label>` always)
- `.card` (surface, 12px radius, hairline + faint shadow, 24–32px padding)
- `.flash-ok / .flash-err / .flash-info` (with `role="alert"` for errors)
- `.empty-state` (centered icon + title + guidance + action — never a bare table)
- `.skeleton` (shimmer for HTMX swaps), `.table` (tabular numbers, row hover)
- `.stat-card` (dashboard metrics: big number + muted label)
- `:focus-visible` ring on everything interactive; never `outline: none` bare.

## Layout

- Sidebar 240px (grouped: Overview / Manage / Record / System) + sticky blur topbar
  (section label, date, avatar chip) + fluid main (`max-width: 72rem`).
- Mobile: sidebar collapses to a horizontal icon nav; labels appear from `md` up.
- Login is a split screen: dark brand panel (mesh glow + grid + feature rows) + form card.
- Page header pattern: eyebrow + 30px semibold title + muted subtitle + right-aligned pill/action.
- Card grids: `repeat(auto-fit, minmax(220px, 1fr))`, no media queries needed.
- Mobile-first; verify 360 / 768 / 1280. No horizontal scroll.

## Template convention (Askama `base.html`)

Every page extending `base.html` provides: `section` (nav key), `section_label`
(topbar title), `user_name`, `user_initial` (one capital letter), plus `title` and
`content` blocks. Missing fields fail at compile time — by design.

## Assets (self-hosted, zero CDN dependency)

- `static/css/tailwind.css` — compiled locally via the Tailwind v4 standalone
  binary from `static/css/input.css` (`@source ../templates`). Rebuilt by
  `./dev.sh` on start and watched on save; committed so Docker needs no toolchain.
- `static/js/htmx.min.js` — vendored htmx 2.0.4 (badge polling only; page works
  fully without it). Google Fonts stays CDN with system-font fallback.
- Rationale (impeccable audit): render-blocking third-party scripts caused
  unstyled first paint; owning the critical path fixed it.

## Credits (tell the team)

UI system informed by: the `frontend-design` visual-craft standard (spacing, hierarchy,
restraint); GitHub's most-starred design resources — `VoltAgent/awesome-design-md`
(~120k★, DESIGN.md token pattern) including its **Linear spec** (lavender accent
discipline, hairline borders, surface ladder, 8px buttons, eyebrow labels,
negative-tracked display type — applied in v2 redesign), `shadcn/ui` (117k★,
neutral ramp + radius tokens + focus rings), `ant-design` (98k★, enterprise
form/table discipline), `storybook` (90k★, states-first component thinking), `daisyui`/`chakra-ui` (semantic tokens);
reference data model: `yungifez/skuul` (MIT) with `4jean/lav_sms` (MIT) as secondary
inspiration for marksheets/receipts.
