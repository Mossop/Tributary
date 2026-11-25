# Dashboard

Unified task dashboard: Rust (axum) server in `server/`, Lit + Web Awesome
frontend in `frontend/`. See README.md for features and configuration.

## Tooling

- Use mise tasks (`mise tasks`): `test`, `lint`, `fmt`, `build`, `dev:server`,
  `dev:frontend`, `docker:build`, `docker:run`. Pass `-C <repo root>` rather
  than changing directory.
- Version control is jujutsu (`jj`), colocated with git.

## Server layout (`server/src`)

- `config.rs` — TOML + `DASHBOARD_` env config via figment, plus validation.
- `sources/mod.rs` — the `Source` trait contract and `build_source` registry.
  `sources/github.rs`, `sources/stub.rs` are implementations.
- `db.rs` — SQLite (sqlx, runtime queries, migrations in `server/migrations`).
  Timestamps are stored as epoch milliseconds. Query functions take `now`
  explicitly. Sorting lives in `list_items`.
- `scheduler.rs` — one polling task per source; `SourceHandle` forces refreshes.
- `activity.rs` — active/idle tracking from client pings.
- `events.rs` — broadcast events streamed over SSE at `/api/events`.
- `api.rs` — HTTP routes; also serves `static_dir` with SPA fallback.
- `lib.rs` — `Workspace` ties config, db, events and running sources together.
  It is the seam for future per-user state.

## Frontend layout (`frontend/src`)

- `api.ts` — typed API client; `store.ts` — state, SSE subscription, activity
  pings; `route.ts` — `/c/<context>[/snoozed]` URLs; `format.ts` — date helpers.
- `components/` — `dash-app` (shell), `item-row`, `date-dialog`.
- `icons.ts` registers Material Symbols as Web Awesome's default icon library
  (served from node_modules in dev, copied to `dist/material-symbols` on build).
- Style: `let` everywhere except module-level constants (ESLint `prefer-const`
  is off for this reason); `==` unless coercion matters. Import with `.ts`
  extensions.

## Testing

- Server integration tests in `server/tests/` use `common::start`, which
  replaces every configured source with a controllable `TestSource`.
- Real sources are tested by intercepting HTTP with wiremock (see
  `tests/github.rs`); sources take a configurable API base URL for this.
- Scheduler tests use real time with millisecond intervals.
- Frontend tests are Vitest + happy-dom. `test-helpers.ts` has `mockFetch` and
  fixtures. Component tests `vi.mock` the Web Awesome component modules
  because happy-dom lacks form-associated custom element support.

## Scope notes

- Quick actions and creating items are deliberately not implemented yet.
- Sources do not have configurable icons; a source defines its icon and items
  may optionally override it.
