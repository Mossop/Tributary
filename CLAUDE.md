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

## Testing

- Server integration tests in `server/tests/` use `common::start`, which
  replaces every configured source with a controllable `TestSource`.
- Real sources are tested by intercepting HTTP with wiremock (see
  `tests/github.rs`); sources take a configurable API base URL for this.
- Scheduler tests use real time with millisecond intervals.

## Scope notes

- Quick actions and creating items are deliberately not implemented yet.
- Sources do not have configurable icons; a source defines its icon and items
  may optionally override it.
