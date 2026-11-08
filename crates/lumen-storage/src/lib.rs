//! Append-only session journal. Only acknowledged playback projects into audible dialogue.
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{path::Path, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub sequence: i64,
    pub monotonic_ms: u64,
    pub kind: String,
    pub payload: String,
}

pub struct Journal {
    pool: SqlitePool,
}

impl Journal {
    pub async fn open(path: &Path) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(Duration::from_secs(3));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::raw_sql("CREATE TABLE IF NOT EXISTS events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            session TEXT NOT NULL CHECK(length(session) BETWEEN 1 AND 128),
            monotonic_ms INTEGER NOT NULL CHECK(monotonic_ms >= 0),
            kind TEXT NOT NULL CHECK(kind IN ('queued','played','cancelled','control','failure')),
            payload TEXT NOT NULL CHECK(length(CAST(payload AS BLOB)) <= 65536)
        );
        CREATE INDEX IF NOT EXISTS events_session_sequence ON events(session, sequence);
        CREATE TRIGGER IF NOT EXISTS events_no_update BEFORE UPDATE ON events BEGIN SELECT RAISE(ABORT, 'append-only journal'); END;
        CREATE TRIGGER IF NOT EXISTS events_no_delete BEFORE DELETE ON events BEGIN SELECT RAISE(ABORT, 'append-only journal'); END;")
            .execute(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn append(
        &self,
        session: &str,
        monotonic_ms: u64,
        kind: &str,
        payload: &str,
    ) -> Result<i64, sqlx::Error> {
        if session.trim().is_empty() || session.len() > 128 || payload.len() > 65536 {
            return Err(sqlx::Error::Protocol("invalid journal input".into()));
        }
        let timestamp = i64::try_from(monotonic_ms)
            .map_err(|_| sqlx::Error::Protocol("timestamp overflow".into()))?;
        let result =
            sqlx::query("INSERT INTO events(session,monotonic_ms,kind,payload) VALUES(?,?,?,?)")
                .bind(session)
                .bind(timestamp)
                .bind(kind)
                .bind(payload)
                .execute(&self.pool)
                .await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn events(
        &self,
        session: &str,
        after: i64,
        limit: u32,
    ) -> Result<Vec<Event>, sqlx::Error> {
        if limit == 0 || limit > 1000 || after < 0 {
            return Err(sqlx::Error::Protocol("invalid journal page".into()));
        }
        let rows = sqlx::query("SELECT sequence,monotonic_ms,kind,payload FROM events WHERE session=? AND sequence>? ORDER BY sequence LIMIT ?")
            .bind(session).bind(after).bind(limit).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(Event {
                    sequence: row.try_get("sequence")?,
                    monotonic_ms: row.try_get::<i64, _>("monotonic_ms")? as u64,
                    kind: row.try_get("kind")?,
                    payload: row.try_get("payload")?,
                })
            })
            .collect()
    }

    pub async fn recent_audible(
        &self,
        session: &str,
        limit: u32,
    ) -> Result<Vec<String>, sqlx::Error> {
        if !(1..=32).contains(&limit) {
            return Err(sqlx::Error::Protocol("invalid context limit".into()));
        }
        let mut rows: Vec<String> = sqlx::query_scalar("SELECT payload FROM events WHERE session=? AND kind='played' ORDER BY sequence DESC LIMIT ?")
            .bind(session).bind(limit).fetch_all(&self.pool).await?;
        rows.reverse();
        Ok(rows)
    }

    pub async fn audible(&self, session: &str) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT payload FROM events WHERE session=? AND kind='played' ORDER BY sequence",
        )
        .bind(session)
        .fetch_all(&self.pool)
        .await
    }
}
