//! Storage for contexts and sources.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, SqlitePool};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct Context {
    /// The slug used in URLs.
    pub id: String,
    pub name: String,
    /// Material Symbol name.
    #[serde(default)]
    pub icon: Option<String>,
}

pub const DEFAULT_CONTEXT: &str = "default";

/// A configured source.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceRecord {
    pub id: String,
    /// The source type id from the registry.
    pub kind: String,
    pub name: String,
    /// Ids of the contexts this source is displayed in.
    pub contexts: Vec<String>,
    /// Settings specific to the source type.
    pub config: Value,
}

pub async fn list_contexts(pool: &SqlitePool) -> sqlx::Result<Vec<Context>> {
    sqlx::query_as("SELECT id, name, icon FROM contexts ORDER BY position, rowid")
        .fetch_all(pool)
        .await
}

pub async fn get_context(pool: &SqlitePool, id: &str) -> sqlx::Result<Option<Context>> {
    sqlx::query_as("SELECT id, name, icon FROM contexts WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Adds a context after the existing ones.
pub async fn insert_context(pool: &SqlitePool, context: &Context) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO contexts (id, name, icon, position)
        VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(position), 0) + 1 FROM contexts))
        "#,
    )
    .bind(&context.id)
    .bind(&context.name)
    .bind(&context.icon)
    .execute(pool)
    .await?;
    Ok(())
}

/// Updates a context, possibly changing its id. Returns false if there was
/// no context with `id`.
pub async fn update_context(pool: &SqlitePool, id: &str, context: &Context) -> sqlx::Result<bool> {
    let result = sqlx::query("UPDATE contexts SET id = ?1, name = ?2, icon = ?3 WHERE id = ?4")
        .bind(&context.id)
        .bind(&context.name)
        .bind(&context.icon)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_context(pool: &SqlitePool, id: &str) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM contexts WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Names of the sources whose only context is `id`.
pub async fn sources_only_in_context(pool: &SqlitePool, id: &str) -> sqlx::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT s.name FROM sources s
        JOIN source_contexts sc ON sc.source_id = s.id
        GROUP BY s.id
        HAVING COUNT(*) = 1 AND MAX(sc.context_id) = ?1
        ORDER BY s.name
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(name,)| name).collect())
}

/// Creates the default context if there are no contexts.
pub async fn ensure_default_context(pool: &SqlitePool) -> sqlx::Result<()> {
    if list_contexts(pool).await?.is_empty() {
        insert_context(
            pool,
            &Context {
                id: DEFAULT_CONTEXT.to_string(),
                name: "Default".to_string(),
                icon: None,
            },
        )
        .await?;
    }
    Ok(())
}

#[derive(FromRow)]
struct SourceRow {
    id: String,
    #[sqlx(rename = "type")]
    kind: String,
    name: String,
    config: String,
    contexts: Option<String>,
}

impl From<SourceRow> for SourceRecord {
    fn from(row: SourceRow) -> Self {
        SourceRecord {
            id: row.id,
            kind: row.kind,
            name: row.name,
            contexts: row
                .contexts
                .and_then(|c| serde_json::from_str(&c).ok())
                .unwrap_or_default(),
            config: serde_json::from_str(&row.config).unwrap_or(Value::Null),
        }
    }
}

macro_rules! select_sources {
    ($rest:literal) => {
        concat!(
            r#"
            SELECT s.id, s.type, s.name, s.config,
                   (SELECT json_group_array(sc.context_id) FROM source_contexts sc
                    WHERE sc.source_id = s.id) AS contexts
            FROM sources s
            "#,
            $rest
        )
    };
}

pub async fn list_sources(pool: &SqlitePool) -> sqlx::Result<Vec<SourceRecord>> {
    let rows: Vec<SourceRow> = sqlx::query_as(select_sources!("ORDER BY s.name, s.id"))
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(SourceRecord::from).collect())
}

pub async fn get_source(pool: &SqlitePool, id: &str) -> sqlx::Result<Option<SourceRecord>> {
    let row: Option<SourceRow> = sqlx::query_as(select_sources!("WHERE s.id = ?1"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(SourceRecord::from))
}

/// Ids of the sources shown in a context.
pub async fn context_source_ids(pool: &SqlitePool, context: &str) -> sqlx::Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT source_id FROM source_contexts WHERE context_id = ?1")
            .bind(context)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Inserts or replaces a source and its contexts.
pub async fn save_source(pool: &SqlitePool, source: &SourceRecord) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO sources (id, type, name, config)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT (id) DO UPDATE SET
            type = excluded.type,
            name = excluded.name,
            config = excluded.config
        "#,
    )
    .bind(&source.id)
    .bind(&source.kind)
    .bind(&source.name)
    .bind(source.config.to_string())
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM source_contexts WHERE source_id = ?1")
        .bind(&source.id)
        .execute(&mut *tx)
        .await?;
    for context in &source.contexts {
        sqlx::query("INSERT INTO source_contexts (source_id, context_id) VALUES (?1, ?2)")
            .bind(&source.id)
            .bind(context)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

/// Deletes a source along with its items, item state and status.
pub async fn delete_source(pool: &SqlitePool, id: &str) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    for query in [
        "DELETE FROM items WHERE source_id = ?1",
        "DELETE FROM item_state WHERE source_id = ?1",
        "DELETE FROM source_status WHERE source_id = ?1",
    ] {
        sqlx::query(query).bind(id).execute(&mut *tx).await?;
    }
    let result = sqlx::query("DELETE FROM sources WHERE id = ?1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::db::connect_in_memory;

    fn context(id: &str) -> Context {
        Context {
            id: id.to_string(),
            name: id.to_uppercase(),
            icon: None,
        }
    }

    fn source(id: &str, contexts: &[&str]) -> SourceRecord {
        SourceRecord {
            id: id.to_string(),
            kind: "stub".to_string(),
            name: format!("Source {id}"),
            contexts: contexts.iter().map(|c| c.to_string()).collect(),
            config: json!({ "x": 1 }),
        }
    }

    #[tokio::test]
    async fn default_context_created_only_when_empty() {
        let pool = connect_in_memory().await.unwrap();
        ensure_default_context(&pool).await.unwrap();
        ensure_default_context(&pool).await.unwrap();
        let contexts = list_contexts(&pool).await.unwrap();
        assert_eq!(
            contexts,
            [Context {
                id: "default".to_string(),
                name: "Default".to_string(),
                icon: None
            }]
        );
    }

    #[tokio::test]
    async fn contexts_keep_insertion_order() {
        let pool = connect_in_memory().await.unwrap();
        for id in ["b", "a", "c"] {
            insert_context(&pool, &context(id)).await.unwrap();
        }
        let ids: Vec<_> = list_contexts(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, ["b", "a", "c"]);
    }

    #[tokio::test]
    async fn renaming_context_updates_sources() {
        let pool = connect_in_memory().await.unwrap();
        insert_context(&pool, &context("a")).await.unwrap();
        save_source(&pool, &source("s", &["a"])).await.unwrap();

        assert!(update_context(&pool, "a", &context("z")).await.unwrap());
        assert!(
            !update_context(&pool, "missing", &context("y"))
                .await
                .unwrap()
        );
        assert_eq!(
            get_source(&pool, "s").await.unwrap().unwrap().contexts,
            ["z"]
        );
        assert_eq!(context_source_ids(&pool, "z").await.unwrap(), ["s"]);
    }

    #[tokio::test]
    async fn round_trips_sources() {
        let pool = connect_in_memory().await.unwrap();
        insert_context(&pool, &context("a")).await.unwrap();
        insert_context(&pool, &context("b")).await.unwrap();

        let mut s = source("s", &["a", "b"]);
        save_source(&pool, &s).await.unwrap();
        let mut stored = get_source(&pool, "s").await.unwrap().unwrap();
        stored.contexts.sort();
        assert_eq!(stored, s);

        s.name = "Renamed".to_string();
        s.contexts = vec!["b".to_string()];
        s.config = json!({ "y": true });
        save_source(&pool, &s).await.unwrap();
        assert_eq!(list_sources(&pool).await.unwrap(), [s]);
        assert!(context_source_ids(&pool, "a").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn finds_sources_only_in_a_context() {
        let pool = connect_in_memory().await.unwrap();
        insert_context(&pool, &context("a")).await.unwrap();
        insert_context(&pool, &context("b")).await.unwrap();
        save_source(&pool, &source("only-a", &["a"])).await.unwrap();
        save_source(&pool, &source("both", &["a", "b"]))
            .await
            .unwrap();
        save_source(&pool, &source("only-b", &["b"])).await.unwrap();
        assert_eq!(
            sources_only_in_context(&pool, "a").await.unwrap(),
            ["Source only-a"]
        );
    }

    #[tokio::test]
    async fn deleting_context_removes_it_from_sources() {
        let pool = connect_in_memory().await.unwrap();
        insert_context(&pool, &context("a")).await.unwrap();
        insert_context(&pool, &context("b")).await.unwrap();
        save_source(&pool, &source("s", &["a", "b"])).await.unwrap();
        assert!(delete_context(&pool, "a").await.unwrap());
        assert_eq!(
            get_source(&pool, "s").await.unwrap().unwrap().contexts,
            ["b"]
        );
    }

    #[tokio::test]
    async fn deleting_source_removes_its_data() {
        let pool = connect_in_memory().await.unwrap();
        insert_context(&pool, &context("a")).await.unwrap();
        save_source(&pool, &source("s", &["a"])).await.unwrap();
        crate::db::replace_items(&pool, "s", &[], chrono::Utc::now())
            .await
            .unwrap();

        assert!(delete_source(&pool, "s").await.unwrap());
        assert!(!delete_source(&pool, "s").await.unwrap());
        assert!(get_source(&pool, "s").await.unwrap().is_none());
        assert!(crate::db::source_statuses(&pool).await.unwrap().is_empty());
        assert!(context_source_ids(&pool, "a").await.unwrap().is_empty());
    }
}
