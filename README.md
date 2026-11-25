# Dashboard

A self-hosted dashboard that collects items of interest (pull requests, bugs,
emails, tasks…) from many **sources** into one prioritised list, so you can see
everything you need to do in one place.

- **Contexts** are separate views (e.g. work and personal), each at its own URL
  (`/c/<context>`). Each source is shown in one or more contexts. A switcher in
  the header moves between them.
- Items are sorted with those that have a due date first (soonest first),
  followed by the rest, least recently modified first.
- Items can be **snoozed** until a date and time, or forever. The snoozed view
  (`/c/<context>/snoozed`) lists them and lets you unsnooze them.
- An item's **due date can be overridden**.
- Sources are polled on their own schedule, more slowly while nobody is using
  the dashboard. Opening the dashboard refreshes everything, and there is a
  refresh button. Updates are pushed to the browser as they arrive.

Currently available sources: GitHub (issue/PR search) and a stub source with
fixed items.

## Running

Copy `config.example.toml` to `config.toml` and edit it (see
[Configuration](#configuration)).

### Docker

```sh
mise run docker:build
GITHUB_TOKEN=... mise run docker:run
```

`docker:run` mounts `./config.toml` at `/config/config.toml` and stores the
database in `./data`. In the image the database path and frontend directory
are preset with environment variables, so they don't need setting in the
config file.

### Locally

```sh
mise install
GITHUB_TOKEN=... mise run dev:server   # API and built frontend on :8080
mise run dev:frontend                  # Vite dev server, proxies /api to :8080
```

`dev:server` serves `frontend/dist`, so run `mise run build:frontend` first
if you want the built frontend instead of the Vite dev server.

## Configuration

A TOML file (default `config.toml`, change it with `--config`). Any value can
be overridden with environment variables prefixed `DASHBOARD_`, with `__`
between nested keys, e.g. `DASHBOARD_SERVER__LISTEN=127.0.0.1:9000`.

```toml
[server]                        # all optional
listen = "0.0.0.0:8080"
database = "data/dashboard.db"
static_dir = "frontend/dist"
active_timeout = "5m"           # idle after this long with no client activity

[[contexts]]
id = "work"                     # used in the URL
name = "Work"
icon = "work"                   # optional Material Symbol name

[[sources]]
id = "my-prs"                   # unique, stable: user state is keyed on it
type = "github"
name = "My pull requests"
contexts = ["work"]
interval = "5m"                 # polling while active (default 5m)
idle_interval = "30m"           # polling while idle (default 30m)
# ...plus settings for the source type
```

### `github`

| Key         | Default                  | Description                                   |
| ----------- | ------------------------ | --------------------------------------------- |
| `query`     | required                 | A GitHub issue search, e.g. `is:open is:pr author:@me` |
| `token_env` | `GITHUB_TOKEN`           | Environment variable holding an API token     |
| `api_url`   | `https://api.github.com` | API base URL (e.g. for GitHub Enterprise)     |

Shows up to 100 results, most recently updated first.

### `stub`

Fixed items listed in `[[sources.items]]` tables with `id`, `title`, `url`,
`modified` and optionally `description`, `due` and `icon`.

## Adding a source

Sources implement the `Source` trait in `server/src/sources/mod.rs`:

```rust
#[async_trait]
pub trait Source: Send + Sync {
    /// URL, or `symbol:<name>` for a Material Symbol.
    fn icon(&self) -> String;
    /// The complete current list of items.
    async fn fetch(&self) -> Result<Vec<SourceItem>, SourceError>;
}
```

Each `SourceItem` has a stable `id`, `title`, `url`, `modified`, and optional
`description`, `due` and `icon` (used instead of the source's icon).

1. Add a module under `server/src/sources/` with a `Deserialize` config struct
   and the `Source` implementation.
2. Add a variant to `SourceKind` in `server/src/config.rs`. Its lowercase name
   is the `type` in configuration.
3. Add a match arm in `build_source`.
4. Test it by intercepting its HTTP calls with wiremock (see
   `server/tests/github.rs`). Give the source a configurable API base URL so
   tests can point it at the mock server.

## Development

Run `mise tasks` to see all tasks. The main ones:

- `mise run test` runs the Rust and frontend unit tests.
- `mise run lint` runs rustfmt, clippy, tsc, eslint and prettier checks.
- `mise run fmt` formats everything.

The server is Rust (axum, tokio, sqlx/SQLite). The frontend is TypeScript with
Lit and [Web Awesome](https://webawesome.com) components, using
[Material Symbols](https://fonts.google.com/icons) for icons.
