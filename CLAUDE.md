# Tributary

Unified task dashboard: Rust (axum) server in `server/`, Lit + Web Awesome
frontend in `frontend/`. See README.md for features and configuration.

## Tooling

- Use mise tasks (`mise tasks`): `test`, `lint`, `fmt`, `build`, `dev:server`,
  `dev:frontend`, `docker:build`, `docker:run`. Pass `-C <repo root>` rather
  than changing directory.
- Version control is jujutsu (`jj`), colocated with git.

## Server layout (`server/src`)

- `config.rs` — `[server]` and `[polling]` TOML + `TRIBUTARY_` env config via
  figment. Polling intervals are global, not per source or UI configurable.
- `settings.rs` — contexts and sources in SQLite. Context `id` is the URL slug
  (renames cascade). Sources have generated ids, common columns, and a JSON
  `config` for type specific settings.
- `sources/mod.rs` — the `Source` trait, `SourceType` (field schema + build
  fn, secret handling) and `Registry`. `sources/github.rs`, `sources/stub.rs`
  are implementations; stub is unlisted (tests/dev only).
- `db.rs` — items, item state and source status (sqlx runtime queries; sqlx
  0.9 rejects non-literal SQL). Migrations in `server/migrations`. Timestamps
  are epoch milliseconds. Query functions take `now` explicitly. Sorting lives
  in `list_items`.
- `scheduler.rs` — one polling task per source; `SourceHandle` forces
  refreshes; dropping all handles stops the task (results of an in-flight
  fetch are discarded).
- `activity.rs` — active/idle tracking from client pings.
- `events.rs` — broadcast events streamed over SSE at `/api/events`.
- `api/` — HTTP routes (`items.rs`, `settings_api.rs`); also serves
  `static_dir` with SPA fallback.
- `lib.rs` — `Workspace` ties config, db, events, the registry and the running
  sources together; `run`/`stop` start, restart and stop sources at runtime.
  It is the seam for future per-user state.

## Frontend layout (`frontend/src`)

- `api.ts` — typed API client; `store.ts` — state, SSE subscription, activity
  pings; `route.ts` — `/c/<context>[/snoozed]` and `/settings` URLs;
  `format.ts` — date helpers; `settings-form.ts` — pure helpers for the
  settings forms.
- `components/` — `dash-app` (shell), `item-row`, `date-dialog`,
  `settings-page`, `context-dialog`, `source-dialog` (form generated from the
  source type's fields).
- `icons.ts` registers Material Symbols as Web Awesome's default icon library
  (served from node_modules in dev, copied to `dist/material-symbols` on build).
- Style: `let` everywhere except module-level constants (ESLint `prefer-const`
  is off for this reason); `==` unless coercion matters. Import with `.ts`
  extensions.

## Testing

- Server integration tests in `server/tests/` use `common::start`, which
  registers a `test` source type; `add_source(key, contexts)` creates one via
  the API and `source(key)` returns its controllable `TestSource`.
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
