//! `devin-usage agent` — the mode that runs on remote machines.
//!
//! The app scp's this same binary (cross-compiled for the remote's
//! platform) to `~/.devin-usage/devin-usage-agent` and calls:
//!   agent rows <last_row_id>   incremental rows + session meta
//!   agent baseline             full aggregate, for first-time backfill
//!   agent meta                 session titles + projects
//!   agent --version            version check for self-updating
//!
//! Output is a single JSON object on stdout.

use crate::db;
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Serialize)]
struct RowsOut {
    max: i64,
    rows: Vec<serde_json::Value>,
    /// session meta for sids appearing in rows: sid -> {t: title, p: project}
    meta: HashMap<String, MetaOut>,
}

#[derive(Serialize)]
struct MetaOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    t: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    p: Option<String>,
}

fn buckets() -> (impl Fn(i64) -> String, impl Fn(i64) -> String) {
    (
        |ts| {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|d| {
                    d.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d")
                        .to_string()
                })
                .unwrap_or_default()
        },
        |ts| {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|d| d.with_timezone(&chrono::Local).format("%H").to_string())
                .unwrap_or_default()
        },
    )
}

pub fn run_rows(db_path: &Path, last: i64) -> Result<()> {
    let con = db::open(db_path)?;
    let rows = db::fetch_rows(&con, last)?;
    let sids: std::collections::HashSet<&str> =
        rows.iter().map(|r| r.session_id.as_str()).collect();
    let titles = db::fetch_titles(&con).unwrap_or_default();
    let projects = db::fetch_projects(&con).unwrap_or_default();
    let meta: HashMap<String, MetaOut> = sids
        .iter()
        .map(|sid| {
            (
                (*sid).to_string(),
                MetaOut {
                    t: titles.get(*sid).cloned(),
                    p: projects.get(*sid).cloned(),
                },
            )
        })
        .collect();
    let rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!([
                r.session_id,
                r.created_at,
                r.model,
                r.input,
                r.output,
                r.cache_read,
                r.cache_create,
                r.context,
                r.row_id
            ])
        })
        .collect();
    let out = RowsOut {
        max: db::max_row_id(&con)?,
        rows,
        meta,
    };
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}

pub fn run_baseline(db_path: &Path) -> Result<()> {
    let con = db::open(db_path)?;
    let (day, hour) = buckets();
    let m = db::baseline(&con, &day, &hour)?;
    println!("{}", serde_json::to_string(&m)?);
    Ok(())
}

pub fn run_meta(db_path: &Path) -> Result<()> {
    let con = db::open(db_path)?;
    let titles = db::fetch_titles(&con).unwrap_or_default();
    let projects = db::fetch_projects(&con).unwrap_or_default();
    let meta: HashMap<String, MetaOut> = titles
        .keys()
        .chain(projects.keys())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .map(|sid| {
            (
                sid.clone(),
                MetaOut {
                    t: titles.get(sid).cloned(),
                    p: projects.get(sid).cloned(),
                },
            )
        })
        .collect();
    println!("{}", serde_json::to_string(&meta)?);
    Ok(())
}
