# Tributary

Tributary is a self-hosted dashboard that collects items of interest (pull
requests, bugs, emails, tasks…) from many **sources** into one prioritised
list, so you can see everything you need to do in one place.

- **Contexts** are separate views (e.g. work and personal), each at its own URL
  (`/c/<context>`). Each source is shown in one or more contexts. A switcher in
  the header moves between them.
- Items are sorted with those that have a due date first (soonest first),
  followed by the rest, least recently modified first.
- **Related items are grouped.** A bug, the patch fixing it and a task linking
  to either are shown together: the group sits where its highest member
  would, led by an issue (or else a task, then a patch), with the others
  indented beneath. Each keeps its own snooze and due date.
- Every item is a **task**, an **issue** or a **patch** (e.g. a pull request).
  Buttons at the top of the list show or hide each kind; each context
  remembers its choice.
- Items can be **snoozed** until a date and time, or forever. The snoozed view
  (`/c/<context>/snoozed`) lists them and lets you unsnooze them.
- An item's **due date can be overridden**.
- Some items have **quick actions**, such as marking a Google task complete,
  done straight from the list. The item's source is refreshed afterwards.
- Sources are polled on their own schedule, more slowly while nobody is using
  the dashboard. Opening the dashboard refreshes everything, and there is a
  refresh button. Updates are pushed to the browser as they arrive.
- Contexts and sources are managed on the **settings page** (`/settings`, the
  gear button). A dot on the gear means something there needs a look: a
  source that failed to update (its error is shown on its entry) or an
  account that needs reconnecting. Changes take effect immediately, with no server restart. On
  first run there is a single "Default" context, which you can rename.

Currently available source types: GitHub (issue/PR search), Google Tasks,
Phabricator (patches to review, to work on and ready to land) and Bugzilla
(bugs assigned to you, or with flags requested of you).
There is also a stub type with fixed items, used for tests and not offered in
settings.

## Running

Server settings can optionally be put in a config file; copy
`tributary.example.toml` to `tributary.toml` (see [Configuration](#configuration)).

### Docker

```sh
mise run docker:build
mise run docker:run
```

`docker:run` builds the image first if any of its sources changed, then mounts
`./tributary.toml` at `/config/tributary.toml` and stores the database in
`./data`. Use `mise run docker:build --force` to rebuild regardless. In the image the database path and frontend directory
are preset with environment variables, so they don't need setting in the
config file.

### Locally

```sh
mise install
mise run dev            # both of the below, restarted when their sources change
mise run dev:server     # API and built frontend on :8080
mise run dev:frontend   # Vite dev server on :5173, proxies /api to :8080
```

`mise run dev` restarts the server when its code, migrations or
`tributary.toml` change, and Vite when its configuration or dependencies
change (Vite reloads the app itself). Use `mise watch dev:server` or
`mise watch dev:frontend` to watch just one. `dev:server` serves
`frontend/dist`, so run `mise run build:frontend` first if you want the built
frontend instead of the Vite dev server.

## Configuration

Only server and polling settings live in configuration; contexts and sources
are stored in the database. The config file is optional (default
`tributary.toml` in the current directory, change it with `--config`). Any value can be overridden with environment variables
prefixed `TRIBUTARY_`, with `__` between nested keys, e.g.
`TRIBUTARY_SERVER__LISTEN=127.0.0.1:9000`.

```toml
[server]                        # all optional
listen = "0.0.0.0:8080"
database = "data/tributary.sqlite"
static_dir = "frontend/dist"
public_url = "http://localhost:8080"  # where the dashboard is reached

[polling]                       # all optional; applies to every source
active = "5m"                   # while the dashboard is in use
idle = "30m"                    # while it is idle
active_timeout = "5m"           # idle after this long with no client activity

[google]                        # needed to connect Google accounts
client_id = "…apps.googleusercontent.com"
client_secret = "…"             # or set TRIBUTARY_GOOGLE__CLIENT_SECRET
```

### GitHub accounts

GitHub sources use a GitHub account added in the Accounts section of
settings ("Add account" → GitHub) with a
[personal access token](https://github.com/settings/tokens). For private
repositories a classic token needs the `repo` scope; for organisations that
use SSO, also authorize the token for them ("Configure SSO" on the token).
For GitHub Enterprise, give its API URL, e.g.
`https://github.example.com/api/v3`. The account is named after the token's
GitHub login. If GitHub stops accepting the token, the account is marked as
needing reconnecting: use Reconnect to give it a new token.

GitHub uses tokens rather than an OAuth "connect" flow because OAuth apps
can't read private repositories without write access to all of them either,
need an app registered on every GitHub host, and are blocked by
organisations that restrict third-party apps until an owner approves them.

### Phabricator accounts

Phabricator sources use a Phabricator account, added in settings ("Add
account" → Phabricator) with a Conduit API token: in Phabricator, open your
settings, then Conduit API Tokens, and generate one. The account is named
after your Phabricator username. It defaults to Mozilla's Phabricator; give
an API URL (e.g. `https://phabricator.example.com/api`) for another instance.

### Bugzilla accounts

Bugzilla sources use a Bugzilla account, added in settings ("Add account" →
Bugzilla) with an API key: in Bugzilla, open Preferences, then API Keys, and
generate one. The account is named after your Bugzilla login. It defaults to
Mozilla's Bugzilla; give its URL (e.g. `https://bugzilla.example.com`) for
another instance.

### Google accounts

Google sources use a Google account connected on the settings page. One
account can be shared by many sources. To allow connecting accounts:

1. In the [Google Cloud console](https://console.cloud.google.com/), create a
   project and enable the APIs for the sources you'll use (the Google Tasks
   API).
2. Configure the OAuth consent screen, adding yourself as a test user while
   the app is in testing.
3. Create an OAuth client ID of type "Web application" with the authorised
   redirect URI `<public_url>/api/oauth/google/callback`, e.g.
   `http://localhost:8080/api/oauth/google/callback`. Google only allows
   plain `http` for `localhost`; anywhere else needs `https`.
4. Put the client ID and secret in `[google]`, and set `public_url` if the
   dashboard isn't at `http://localhost:8080`.

Then use "Connect Google account" in the Accounts section of settings. If
an account's access is revoked or expires, or a source type needing more
access is added later, the account is marked as needing reconnecting and a
dot appears on the settings button.

### Source settings

Every source has a name and the contexts it's shown in. Each source type adds
its own settings:

**GitHub search**: a GitHub account and a search query (e.g.
`is:open is:pr author:@me`). Shows up to 100 results, most recently updated
first.

**Google Tasks**: a Google account and a task list, or all lists. Shows
incomplete tasks; with all lists, each task's description starts with its
list's name. Google only stores the date of a due time, so tasks are due at
midday UTC on that date. Tasks have a "Mark complete" quick action, which
needs read and write access to Google Tasks; accounts connected before it was
added show "Needs reconnecting" until reconnected.

**Phabricator**: a Phabricator account and which patches to show:

- **Patches to review**: revisions needing review where review is asked of
  you, or of a group you're in. Revisions you've already accepted or
  requested changes on are left out until they're updated. A group's request
  is ignored when another member of that group is a reviewer themselves (and
  hasn't resigned). Your own revisions are left out.
- **Patches to work on**: your revisions that are drafts, need revision or
  have changes planned.
- **Patches ready to land**: your accepted revisions, each with an "Open in
  Lando" quick action. Lando has no API to tell whether a revision is already
  queued for landing, so queued revisions stay listed until they land and
  Phabricator closes them.

**Bugzilla**: a Bugzilla account and what to show:

- **Bugs assigned to me**: your open bugs.
- **Requests: _flag_**: bugs with that flag (e.g. needinfo) requested of you,
  whether on the bug or on one of its attachments (except obsolete ones),
  including closed bugs. Each item says who asked and is dated by when you
  were asked. The flags offered are needinfo plus any requested of you
  recently. **Requests: any flag** lists every request of you.

Account tokens and secret source settings are stored in the database in
plain text. They are never sent back to the browser; leave a secret field
blank when editing to keep the saved value.

## Adding a source type

Sources implement the `Source` trait in `server/src/sources/mod.rs`:

```rust
#[async_trait]
pub trait Source: Send + Sync {
    /// URL, or `symbol:<name>` for a Material Symbol.
    fn icon(&self) -> String;
    /// The complete current list of items.
    async fn fetch(&self) -> Result<Vec<SourceItem>, SourceError>;
    /// Performs one of an item's quick actions (optional).
    async fn perform(&self, item_id: &str, action: &str) -> Result<(), SourceError>;
}
```

Each `SourceItem` has a stable `id`, a `kind` (`ItemKind::Task`, `Issue` or
`Patch`), `title`, `url`, `modified`, and optional
`description`, `due`, `icon` (used instead of the source's icon) and
`actions`. Each quick action has an `id` passed to `perform`, a `label` and
an `icon`; the dashboard refreshes the source after performing one. A quick
action with a `url` is a link instead: the dashboard opens it in a new tab
and `perform` isn't called. An item's
`id` should contain whatever `perform` needs, e.g. `list/task` for Google
Tasks.

An item's `references` name what it's about, including itself; items
sharing any are grouped, transitively. Use the helpers in
`server/src/sources/references.rs`: `bugzilla:<id>` (Mozilla's Bugzilla),
`phab:D<id>` and `github:<owner>/<repo>:<number>`. `references::in_text`
finds them as links in free text. Currently:

- Bugzilla bugs reference themselves.
- Phabricator revisions reference themselves and their bug (Mozilla's bug
  field, or a title starting "Bug N").
- GitHub issues and pull requests reference themselves; pull requests also
  reference the issues they close ("Fixes #12").
- Google tasks reference any Bugzilla, Phabricator or GitHub links in their
  title or notes.

1. Add a module under `server/src/sources/` with a `Deserialize` config struct
   and the `Source` implementation.
2. Describe the type with a `SourceType`: an id, a display name, its settings
   as a list of `Field`s (key, label, kind, required, placeholder, help), and
   a function building the source from its JSON config. The settings page
   builds its form from the fields, so no frontend changes are needed. See
   `github::source_type()`. Field kinds are:
   - `text`, `url` and `secret` (write-only) inputs.
   - `account`: a connected account of the field's `provider`. Declare the
     OAuth scopes the source needs with `with_scope`, and get access tokens
     at fetch time from the `SourceContext`'s `accounts`.
   - `select`: a dropdown whose options come from the function given to
     `with_options`, which gets the rest of the config (e.g. the chosen
     account). See `google_tasks::source_type()`.
3. Register it in `Registry::builtin()`.
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
