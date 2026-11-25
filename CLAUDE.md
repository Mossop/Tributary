# Tributary

Unified task dashboard: Rust (axum) server in `server/`, Lit + Web Awesome
frontend in `frontend/`. See README.md for features and configuration.

## Tooling

- Use mise tasks (`mise tasks`): `test`, `lint`, `fmt`, `build`, `dev` (both
  dev servers under `mise watch`), `dev:server`, `dev:frontend`,
  `docker:build` (skipped when its `sources` are unchanged), `docker:run`.
  Keep task `sources` up to date when adding files the tasks depend on. Pass `-C <repo root>` rather
  than changing directory.
- Version control is jujutsu (`jj`), colocated with git.

## Server layout (`server/src`)

- `config.rs` — `[server]`, `[polling]` and `[google]` TOML + `TRIBUTARY_` env config via
  figment. Polling intervals are global, not per source or UI configurable.
- `settings.rs` — contexts and sources in SQLite. Context `id` is the URL slug
  (renames cascade). Sources have generated ids, common columns, and a JSON
  `config` for type specific settings.
- `sources/mod.rs` — the `Source` trait, `SourceType` (field schema + build
  fn, secret handling, OAuth scopes, select options fn) and `Registry`.
  `sources/github.rs`, `sources/google_tasks.rs`, `sources/phabricator/`
  (`conduit.rs` is the Conduit client; the review rules are in
  `review_reason`) and `sources/stub.rs` are implementations; stub is
  unlisted (tests/dev only).
- `accounts.rs` — connected accounts in SQLite (`secret` is a Google refresh
  token or a GitHub access token and never leaves the server) and
  `Accounts`: Google access tokens, GitHub credentials, revocations and
  pending OAuth states. Sources get it through `SourceContext`. Google
  accounts connect by OAuth; GitHub accounts are added with a token
  (`POST /api/accounts/github`), deliberately not OAuth (see README).
- `google.rs` — Google OAuth client (consent URL, code exchange, refresh,
  userinfo). Endpoints are configurable so tests use wiremock.
- `db.rs` — items, item state and source status (sqlx runtime queries; sqlx
  0.9 rejects non-literal SQL). Migrations in `server/migrations`; add a new
  one per schema change and never edit existing ones. Timestamps are epoch
  milliseconds. Query functions take `now` explicitly. Sorting lives in
  `list_items`.
- `scheduler.rs` — one polling task per source; `SourceHandle` forces
  refreshes; dropping all handles stops the task (results of an in-flight
  fetch are discarded).
- `activity.rs` — active/idle tracking from client pings.
- `events.rs` — broadcast events streamed over SSE at `/api/events`. The
  streams end on `Workspace::shutdown` so graceful shutdown doesn't hang.
- `api/` — HTTP routes (`items.rs`, `settings_api.rs`, `accounts_api.rs`
  including the OAuth connect/callback redirects); also serves
  `static_dir` with SPA fallback.
- `lib.rs` — `Workspace` ties config, db, events, the registry and the running
  sources together; `run`/`stop` start, restart and stop sources at runtime.
  It is the seam for future per-user state.

## Frontend layout (`frontend/src`)

- `api.ts` — typed API client; `store.ts` — state, SSE subscription, activity
  pings; `route.ts` — `/c/<context>[/snoozed]` and `/settings` URLs;
  `format.ts` — date helpers; `settings-form.ts` — pure helpers for the
  settings forms.
- `components/` — `app-shell` (shell), `item-row`, `date-dialog`,
  `settings-page`, `context-dialog`, `source-dialog` (form generated from the
  source type's fields), `confirm-dialog` (asks before an action and shows its
  error; use it rather than `window.confirm`), `token-account-dialog` (token
  entry).
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
  `tests/google.rs` mocks Google's OAuth and Tasks endpoints end to end.
- Scheduler tests use real time with millisecond intervals.
- Frontend tests are Vitest + happy-dom. `test-helpers.ts` has `mockFetch` and
  fixtures. Component tests `vi.mock` the Web Awesome component modules
  because happy-dom lacks form-associated custom element support.

## Scope notes

- Creating items is deliberately not implemented yet. Quick actions exist
  (`SourceItem.actions`, `Source::perform`, `POST
  /api/items/{source}/{item}/actions/{action}`, which refreshes the source
  before responding); Google Tasks "complete" is the only one so far.
- Sources do not have configurable icons; a source defines its icon and items
  may optionally override it.
