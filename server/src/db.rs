//! SQLite storage for fetched items, per-item user state and source status.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{FromRow, SqlitePool};

use crate::sources::{ItemKind, QuickAction, SourceItem};

pub async fn connect(path: &Path) -> anyhow::Result<SqlitePool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);
    let pool = SqlitePoolOptions::new().connect_with(options).await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

/// An in-memory database, used for tests.
pub async fn connect_in_memory() -> anyhow::Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect("sqlite::memory:")
        .await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

fn to_ms(time: DateTime<Utc>) -> i64 {
    time.timestamp_millis()
}

fn from_ms(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).unwrap_or_default()
}

/// An item along with the user's state for it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub source_id: String,
    pub id: String,
    pub kind: ItemKind,
    pub title: String,
    pub url: String,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub modified: DateTime<Utc>,
    /// The due date reported by the source.
    pub due: Option<DateTime<Utc>>,
    /// The user's override of the due date.
    pub due_override: Option<DateTime<Utc>>,
    pub snoozed_until: Option<DateTime<Utc>>,
    pub snoozed_forever: bool,
    pub actions: Vec<QuickAction>,
}

#[derive(FromRow)]
struct ItemRow {
    source_id: String,
    item_id: String,
    kind: String,
    title: String,
    url: String,
    icon: Option<String>,
    description: Option<String>,
    modified: i64,
    due: Option<i64>,
    due_override: Option<i64>,
    snoozed_until: Option<i64>,
    snoozed_forever: bool,
    actions: String,
}

impl From<ItemRow> for Item {
    fn from(row: ItemRow) -> Self {
        Item {
            source_id: row.source_id,
            id: row.item_id,
            kind: ItemKind::parse(&row.kind).unwrap_or(ItemKind::Task),
            title: row.title,
            url: row.url,
            icon: row.icon,
            description: row.description,
            modified: from_ms(row.modified),
            due: row.due.map(from_ms),
            due_override: row.due_override.map(from_ms),
            snoozed_until: row.snoozed_until.map(from_ms),
            snoozed_forever: row.snoozed_forever,
            actions: parse_actions(&row.actions),
        }
    }
}

fn parse_actions(json: &str) -> Vec<QuickAction> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Which items to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    /// Items that are not currently snoozed.
    Active,
    /// Items that are currently snoozed.
    Snoozed,
}

/// Lists items from the given sources.
///
/// Items with a due date (the override if set, otherwise the source's) come
/// first, soonest first, followed by items without a due date, least recently
/// modified first.
pub async fn list_items(
    pool: &SqlitePool,
    source_ids: &[&str],
    filter: Filter,
    now: DateTime<Utc>,
) -> sqlx::Result<Vec<Item>> {
    let source_ids = serde_json::to_string(source_ids).expect("ids serialize");
    let rows: Vec<ItemRow> = sqlx::query_as(
        r#"
        SELECT i.source_id, i.item_id, i.kind, i.title, i.url, i.icon, i.description,
               i.modified, i.due, s.due_override, s.snoozed_until,
               COALESCE(s.snoozed_forever, 0) AS snoozed_forever, i.actions
        FROM items i
        LEFT JOIN item_state s ON s.source_id = i.source_id AND s.item_id = i.item_id
        WHERE i.source_id IN (SELECT value FROM json_each(?1))
          AND (COALESCE(s.snoozed_forever, 0) = 1 OR COALESCE(s.snoozed_until, 0) > ?2) = ?3
        ORDER BY COALESCE(s.due_override, i.due) IS NULL,
                 COALESCE(s.due_override, i.due),
                 i.modified,
                 i.source_id,
                 i.item_id
        "#,
    )
    .bind(source_ids)
    .bind(to_ms(now))
    .bind(filter == Filter::Snoozed)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Item::from).collect())
}

pub async fn item_exists(pool: &SqlitePool, source_id: &str, item_id: &str) -> sqlx::Result<bool> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM items WHERE source_id = ?1 AND item_id = ?2")
            .bind(source_id)
            .bind(item_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}

/// The quick actions a stored item offers, or None if there is no such item.
pub async fn item_actions(
    pool: &SqlitePool,
    source_id: &str,
    item_id: &str,
) -> sqlx::Result<Option<Vec<QuickAction>>> {
    let actions: Option<String> =
        sqlx::query_scalar("SELECT actions FROM items WHERE source_id = ?1 AND item_id = ?2")
            .bind(source_id)
            .bind(item_id)
            .fetch_optional(pool)
            .await?;
    Ok(actions.as_deref().map(parse_actions))
}

/// Replaces all stored items for a source and records a successful fetch.
pub async fn replace_items(
    pool: &SqlitePool,
    source_id: &str,
    items: &[SourceItem],
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM items WHERE source_id = ?1")
        .bind(source_id)
        .execute(&mut *tx)
        .await?;
    for item in items {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO items
                (source_id, item_id, kind, title, url, icon, description, modified, due, actions)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
        )
        .bind(source_id)
        .bind(&item.id)
        .bind(item.kind.as_str())
        .bind(&item.title)
        .bind(&item.url)
        .bind(&item.icon)
        .bind(&item.description)
        .bind(to_ms(item.modified))
        .bind(item.due.map(to_ms))
        .bind(serde_json::to_string(&item.actions).expect("actions serialize"))
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        r#"
        INSERT INTO source_status (source_id, last_attempt, last_success, last_error)
        VALUES (?1, ?2, ?2, NULL)
        ON CONFLICT (source_id) DO UPDATE SET
            last_attempt = excluded.last_attempt,
            last_success = excluded.last_success,
            last_error = NULL
        "#,
    )
    .bind(source_id)
    .bind(to_ms(now))
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Records a failed fetch. Previously fetched items are kept.
pub async fn record_failure(
    pool: &SqlitePool,
    source_id: &str,
    error: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO source_status (source_id, last_attempt, last_error)
        VALUES (?1, ?2, ?3)
        ON CONFLICT (source_id) DO UPDATE SET
            last_attempt = excluded.last_attempt,
            last_error = excluded.last_error
        "#,
    )
    .bind(source_id)
    .bind(to_ms(now))
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SourceStatus {
    pub last_attempt: Option<DateTime<Utc>>,
    pub last_success: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

#[derive(FromRow)]
struct SourceStatusRow {
    source_id: String,
    last_attempt: Option<i64>,
    last_success: Option<i64>,
    last_error: Option<String>,
}

pub async fn source_statuses(pool: &SqlitePool) -> sqlx::Result<HashMap<String, SourceStatus>> {
    let rows: Vec<SourceStatusRow> = sqlx::query_as(
        "SELECT source_id, last_attempt, last_success, last_error FROM source_status",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.source_id,
                SourceStatus {
                    last_attempt: row.last_attempt.map(from_ms),
                    last_success: row.last_success.map(from_ms),
                    last_error: row.last_error,
                },
            )
        })
        .collect())
}

/// Snoozes an item until the given time, or forever if `until` is `None`.
pub async fn set_snooze(
    pool: &SqlitePool,
    source_id: &str,
    item_id: &str,
    until: Option<DateTime<Utc>>,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO item_state (source_id, item_id, snoozed_until, snoozed_forever)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT (source_id, item_id) DO UPDATE SET
            snoozed_until = excluded.snoozed_until,
            snoozed_forever = excluded.snoozed_forever
        "#,
    )
    .bind(source_id)
    .bind(item_id)
    .bind(until.map(to_ms))
    .bind(until.is_none())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_snooze(pool: &SqlitePool, source_id: &str, item_id: &str) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE item_state SET snoozed_until = NULL, snoozed_forever = 0
        WHERE source_id = ?1 AND item_id = ?2
        "#,
    )
    .bind(source_id)
    .bind(item_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Sets (or with `None` clears) the user's override of an item's due date.
pub async fn set_due_override(
    pool: &SqlitePool,
    source_id: &str,
    item_id: &str,
    due: Option<DateTime<Utc>>,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO item_state (source_id, item_id, due_override)
        VALUES (?1, ?2, ?3)
        ON CONFLICT (source_id, item_id) DO UPDATE SET
            due_override = excluded.due_override
        "#,
    )
    .bind(source_id)
    .bind(item_id)
    .bind(due.map(to_ms))
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;

    fn time(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, day, 0, 0, 0).unwrap()
    }

    fn item(id: &str, modified: u32, due: Option<u32>) -> SourceItem {
        SourceItem {
            id: id.to_string(),
            kind: ItemKind::Task,
            title: format!("Item {id}"),
            url: format!("https://example.com/{id}"),
            icon: None,
            modified: time(modified),
            description: None,
            due: due.map(time),
            actions: Vec::new(),
        }
    }

    async fn ids(pool: &SqlitePool, sources: &[&str], filter: Filter, now: u32) -> Vec<String> {
        list_items(pool, sources, filter, time(now))
            .await
            .unwrap()
            .into_iter()
            .map(|i| format!("{}/{}", i.source_id, i.id))
            .collect()
    }

    #[tokio::test]
    async fn stores_quick_actions() {
        let pool = connect_in_memory().await.unwrap();
        let action = QuickAction {
            id: "complete".into(),
            label: "Mark complete".into(),
            icon: "symbol:check_circle".into(),
            url: None,
        };
        let with_action = SourceItem {
            actions: vec![action.clone()],
            ..item("1", 1, None)
        };
        replace_items(&pool, "a", &[with_action, item("2", 2, None)], time(1))
            .await
            .unwrap();

        let items = list_items(&pool, &["a"], Filter::Active, time(1))
            .await
            .unwrap();
        assert_eq!(items[0].actions, std::slice::from_ref(&action));
        assert!(items[1].actions.is_empty());
        assert_eq!(
            item_actions(&pool, "a", "1").await.unwrap(),
            Some(vec![action])
        );
        assert_eq!(item_actions(&pool, "a", "2").await.unwrap(), Some(vec![]));
        assert_eq!(item_actions(&pool, "a", "3").await.unwrap(), None);
    }

    #[tokio::test]
    async fn sorts_due_first_then_oldest_modified() {
        let pool = connect_in_memory().await.unwrap();
        replace_items(
            &pool,
            "a",
            &[
                item("new", 20, None),
                item("old", 2, None),
                item("due-late", 25, Some(28)),
            ],
            time(1),
        )
        .await
        .unwrap();
        replace_items(&pool, "b", &[item("due-soon", 1, Some(10))], time(1))
            .await
            .unwrap();

        assert_eq!(
            ids(&pool, &["a", "b"], Filter::Active, 1).await,
            ["b/due-soon", "a/due-late", "a/old", "a/new"]
        );
    }

    #[tokio::test]
    async fn filters_by_source() {
        let pool = connect_in_memory().await.unwrap();
        replace_items(&pool, "a", &[item("1", 1, None)], time(1))
            .await
            .unwrap();
        replace_items(&pool, "b", &[item("2", 1, None)], time(1))
            .await
            .unwrap();
        assert_eq!(ids(&pool, &["b"], Filter::Active, 1).await, ["b/2"]);
        assert!(ids(&pool, &[], Filter::Active, 1).await.is_empty());
    }

    #[tokio::test]
    async fn due_override_changes_order() {
        let pool = connect_in_memory().await.unwrap();
        replace_items(
            &pool,
            "a",
            &[item("1", 1, Some(10)), item("2", 1, None)],
            time(1),
        )
        .await
        .unwrap();
        set_due_override(&pool, "a", "2", Some(time(5)))
            .await
            .unwrap();
        assert_eq!(ids(&pool, &["a"], Filter::Active, 1).await, ["a/2", "a/1"]);

        let items = list_items(&pool, &["a"], Filter::Active, time(1))
            .await
            .unwrap();
        assert_eq!(items[0].due, None);
        assert_eq!(items[0].due_override, Some(time(5)));

        set_due_override(&pool, "a", "2", None).await.unwrap();
        assert_eq!(ids(&pool, &["a"], Filter::Active, 1).await, ["a/1", "a/2"]);
    }

    #[tokio::test]
    async fn snooze_until_expires() {
        let pool = connect_in_memory().await.unwrap();
        replace_items(
            &pool,
            "a",
            &[item("1", 1, None), item("2", 1, None)],
            time(1),
        )
        .await
        .unwrap();
        set_snooze(&pool, "a", "1", Some(time(5))).await.unwrap();

        assert_eq!(ids(&pool, &["a"], Filter::Active, 4).await, ["a/2"]);
        assert_eq!(ids(&pool, &["a"], Filter::Snoozed, 4).await, ["a/1"]);

        let snoozed = list_items(&pool, &["a"], Filter::Snoozed, time(4))
            .await
            .unwrap();
        assert_eq!(snoozed[0].snoozed_until, Some(time(5)));
        assert!(!snoozed[0].snoozed_forever);

        let at_expiry = time(5);
        assert_eq!(
            list_items(&pool, &["a"], Filter::Active, at_expiry)
                .await
                .unwrap()
                .len(),
            2
        );
        assert!(
            list_items(
                &pool,
                &["a"],
                Filter::Snoozed,
                at_expiry - Duration::milliseconds(1)
            )
            .await
            .unwrap()
            .len()
                == 1
        );
    }

    #[tokio::test]
    async fn snooze_forever_and_unsnooze() {
        let pool = connect_in_memory().await.unwrap();
        replace_items(&pool, "a", &[item("1", 1, None)], time(1))
            .await
            .unwrap();
        set_snooze(&pool, "a", "1", None).await.unwrap();
        assert!(ids(&pool, &["a"], Filter::Active, 30).await.is_empty());
        let snoozed = list_items(&pool, &["a"], Filter::Snoozed, time(30))
            .await
            .unwrap();
        assert!(snoozed[0].snoozed_forever);

        clear_snooze(&pool, "a", "1").await.unwrap();
        assert_eq!(ids(&pool, &["a"], Filter::Active, 30).await, ["a/1"]);
        assert!(ids(&pool, &["a"], Filter::Snoozed, 30).await.is_empty());
    }

    #[tokio::test]
    async fn refetch_replaces_items_but_keeps_state() {
        let pool = connect_in_memory().await.unwrap();
        replace_items(
            &pool,
            "a",
            &[item("1", 1, None), item("2", 1, None)],
            time(1),
        )
        .await
        .unwrap();
        set_snooze(&pool, "a", "1", None).await.unwrap();
        set_due_override(&pool, "a", "2", Some(time(9)))
            .await
            .unwrap();

        let mut updated = item("2", 3, None);
        updated.title = "Updated".to_string();
        replace_items(
            &pool,
            "a",
            &[item("1", 2, None), updated, item("3", 1, None)],
            time(2),
        )
        .await
        .unwrap();

        let active = list_items(&pool, &["a"], Filter::Active, time(2))
            .await
            .unwrap();
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].id, "2");
        assert_eq!(active[0].title, "Updated");
        assert_eq!(active[0].due_override, Some(time(9)));
        assert_eq!(ids(&pool, &["a"], Filter::Snoozed, 2).await, ["a/1"]);

        replace_items(&pool, "a", &[], time(3)).await.unwrap();
        assert!(ids(&pool, &["a"], Filter::Active, 3).await.is_empty());
        assert!(!item_exists(&pool, "a", "2").await.unwrap());
    }

    #[tokio::test]
    async fn records_status() {
        let pool = connect_in_memory().await.unwrap();
        record_failure(&pool, "a", "boom", time(1)).await.unwrap();
        let status = &source_statuses(&pool).await.unwrap()["a"];
        assert_eq!(status.last_attempt, Some(time(1)));
        assert_eq!(status.last_success, None);
        assert_eq!(status.last_error.as_deref(), Some("boom"));

        replace_items(&pool, "a", &[], time(2)).await.unwrap();
        let status = &source_statuses(&pool).await.unwrap()["a"];
        assert_eq!(status.last_success, Some(time(2)));
        assert_eq!(status.last_error, None);

        record_failure(&pool, "a", "again", time(3)).await.unwrap();
        let status = &source_statuses(&pool).await.unwrap()["a"];
        assert_eq!(status.last_attempt, Some(time(3)));
        assert_eq!(status.last_success, Some(time(2)));
        assert_eq!(status.last_error.as_deref(), Some("again"));
    }
}
