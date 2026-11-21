//! Read-only access to Devin CLI's `sessions.db`.
//!
//! Everything is incremental: `fetch_rows` selects only `row_id > since`, so
//! polling a multi-GB database costs ~nothing when idle.

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;

/// One metric-bearing assistant message.
#[derive(Debug, Clone)]
pub struct Row {
    pub row_id: i64,
    pub session_id: String,
    /// epoch seconds
    pub created_at: i64,
    pub model: String,
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_create: i64,
    pub context: i64,
}

const METRIC_SQL: &str = r#"
SELECT session_id, created_at,
  json_extract(chat_message,'$.metadata.generation_model'),
  json_extract(chat_message,'$.metadata.metrics.input_tokens'),
  json_extract(chat_message,'$.metadata.metrics.output_tokens'),
  json_extract(chat_message,'$.metadata.metrics.cache_read_tokens'),
  json_extract(chat_message,'$.metadata.metrics.cache_creation_tokens'),
  json_extract(chat_message,'$.metadata.metrics.context_input_tokens'),
  row_id
FROM message_nodes
WHERE json_extract(chat_message,'$.role')='assistant'
  AND json_extract(chat_message,'$.metadata.metrics') IS NOT NULL
"#;

pub fn open(db: &Path) -> Result<Connection> {
    let uri = format!("file:{}?mode=ro", db.display());
    match Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    ) {
        Ok(c) => Ok(c),
        Err(_) => Connection::open(db).context("open sessions.db"),
    }
}

pub fn fetch_rows(con: &Connection, since: i64) -> Result<Vec<Row>> {
    let mut stmt = con.prepare(&format!("{METRIC_SQL} AND row_id > ?1 ORDER BY row_id"))?;
    let rows = stmt.query_map([since], |r| {
        Ok(Row {
            session_id: r.get(0)?,
            created_at: r.get(1)?,
            model: r
                .get::<_, Option<String>>(2)?
                .unwrap_or_else(|| "unknown".into()),
            input: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
            output: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
            cache_read: r.get::<_, Option<i64>>(5)?.unwrap_or(0),
            cache_create: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
            context: r.get::<_, Option<i64>>(7)?.unwrap_or(0),
            row_id: r.get(8)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// session_id -> title (for non-NULL titles).
pub fn fetch_titles(con: &Connection) -> Result<HashMap<String, String>> {
    let mut stmt = con.prepare("SELECT id, title FROM sessions WHERE title IS NOT NULL")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// session_id -> project name (basename of working_directory).
pub fn fetch_projects(con: &Connection) -> Result<HashMap<String, String>> {
    let mut stmt = con.prepare(
        "SELECT id, working_directory FROM sessions WHERE working_directory IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows
        .filter_map(|r| r.ok())
        .map(|(id, dir)| (id, project_name(&dir)))
        .collect())
}

pub fn project_name(working_directory: &str) -> String {
    working_directory
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("unknown")
        .to_string()
}

pub fn max_row_id(con: &Connection) -> Result<i64> {
    Ok(con
        .query_row("SELECT MAX(row_id) FROM message_nodes", [], |r| {
            r.get::<_, Option<i64>>(0)
        })?
        .unwrap_or(0))
}

/// Full-table aggregate used once for backfill (and by `agent baseline`).
pub fn baseline(
    con: &Connection,
    day_of: &dyn Fn(i64) -> String,
    hour_of: &dyn Fn(i64) -> String,
) -> Result<crate::state::MachineState> {
    let titles = fetch_titles(con).unwrap_or_default();
    let projects = fetch_projects(con).unwrap_or_default();
    let mut m = crate::state::MachineState {
        last_row_id: max_row_id(con)?,
        titles,
        projects: projects.clone(),
        hours_date: day_of(chrono::Utc::now().timestamp()),
        ..Default::default()
    };
    for row in fetch_rows(con, 0)? {
        let day = day_of(row.created_at);
        m.fold(
            &day,
            &hour_of(row.created_at),
            &row.session_id,
            &row.model,
            row.input,
            row.output,
            row.cache_read,
            row.cache_create,
            row.context,
            projects.get(&row.session_id).map(|s| s.as_str()),
        );
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn fixture() -> Connection {
        let con = Connection::open_in_memory().unwrap();
        con.execute_batch(
            r#"
            CREATE TABLE sessions (id TEXT PRIMARY KEY, working_directory TEXT, title TEXT);
            CREATE TABLE message_nodes (
                row_id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT, node_id INTEGER, chat_message TEXT, created_at INTEGER);
        "#,
        )
        .unwrap();
        con.execute(
            "INSERT INTO sessions VALUES ('s1','/home/u/parishadbarta','Fix bug')",
            [],
        )
        .unwrap();
        let msg = |in_t: i64, out_t: i64| {
            format!(
                r#"{{"role":"assistant","metadata":{{"generation_model":"swe-2-high",
              "metrics":{{"input_tokens":{in_t},"output_tokens":{out_t},
              "cache_read_tokens":50,"cache_creation_tokens":0,"context_input_tokens":60}}}}}}"#
            )
        };
        con.execute("INSERT INTO message_nodes (session_id,node_id,chat_message,created_at) VALUES ('s1',1,?1,1700000000)",
                    params![msg(100, 10)]).unwrap();
        con.execute("INSERT INTO message_nodes (session_id,node_id,chat_message,created_at) VALUES ('s1',2,'{\"role\":\"user\",\"content\":\"hi\"}',1700000001)", []).unwrap();
        con.execute("INSERT INTO message_nodes (session_id,node_id,chat_message,created_at) VALUES ('s1',3,?1,1700000002)",
                    params![msg(200, 20)]).unwrap();
        con
    }

    #[test]
    fn fetch_rows_only_returns_metric_rows() {
        let con = fixture();
        let rows = fetch_rows(&con, 0).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].input, 100);
        assert_eq!(rows[1].output, 20);
    }

    #[test]
    fn fetch_rows_is_incremental() {
        let con = fixture();
        let last = fetch_rows(&con, 0).unwrap()[0].row_id;
        let rows = fetch_rows(&con, last).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].input, 200);
    }

    #[test]
    fn project_name_strips_path() {
        assert_eq!(project_name("/home/u/parishadbarta"), "parishadbarta");
        assert_eq!(project_name("/a/b/c/"), "c");
        assert_eq!(project_name("repo"), "repo");
    }

    #[test]
    fn baseline_aggregates_fixture() {
        let con = fixture();
        let m = baseline(&con, &|_| "2026-11-21".to_string(), &|_| "10".to_string()).unwrap();
        assert_eq!(m.days["2026-11-21"].in_plus_out(), 330);
        assert_eq!(m.day_sessions["2026-11-21"]["s1"].requests, 2);
        assert_eq!(
            m.day_projects["2026-11-21"]["parishadbarta"].input_tokens,
            300
        );
        assert_eq!(m.titles["s1"], "Fix bug");
    }
}
