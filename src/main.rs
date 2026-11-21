mod agent;
mod aggregate;
mod config;
mod db;
mod engine;
mod paths;
mod remote;
mod state;
mod stats;
mod tailscale;
#[cfg(target_os = "macos")]
mod ui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(
    name = "devin-usage",
    version,
    about = "Menu bar tracker for Devin CLI token usage"
)]
struct Cli {
    /// Path to Devin CLI's sessions.db (defaults to the standard location)
    #[arg(long, global = true)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Agent mode — runs on remote machines; prints JSON to stdout
    Agent {
        #[command(subcommand)]
        mode: AgentMode,
    },
    /// Print the current aggregated usage view as JSON and exit
    Stats,
    /// Rebuild the local baseline from scratch (full db scan, one-time)
    Baseline,
    /// Check connectivity to every configured and discovered remote
    Doctor,
}

#[derive(Subcommand)]
enum AgentMode {
    /// Emit rows newer than <last_row_id> plus session meta
    Rows { last: i64 },
    /// Full aggregated history (one-time backfill)
    Baseline,
    /// session_id -> {title, project} for all sessions
    Meta,
}

fn db_path(cli: &Cli) -> PathBuf {
    cli.db.clone().unwrap_or_else(paths::default_db_path)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.cmd {
        Some(Cmd::Agent { mode }) => match mode {
            AgentMode::Rows { last } => agent::run_rows(&db_path(&cli), *last),
            AgentMode::Baseline => agent::run_baseline(&db_path(&cli)),
            AgentMode::Meta => agent::run_meta(&db_path(&cli)),
        },
        Some(Cmd::Stats) => cmd_stats(&cli),
        Some(Cmd::Baseline) => cmd_baseline(&cli),
        Some(Cmd::Doctor) => cmd_doctor(),
        None => cmd_app(&cli),
    }
}

fn cmd_stats(cli: &Cli) -> Result<()> {
    let engine = engine::Engine::new(db_path(cli));
    let _ = engine.poll_local_once();
    let (st, seen) = engine.snapshot();
    let view = aggregate::View::new(
        &st,
        &seen,
        engine::today_pub(),
        engine::last_n_days(14),
        engine::now_pub(),
    );
    println!("{}", serde_json::to_string_pretty(&view.render())?);
    Ok(())
}

fn cmd_baseline(cli: &Cli) -> Result<()> {
    let engine = engine::Engine::new(db_path(cli));
    let con = db::open(&db_path(cli))?;
    let m = db::baseline(&con, &engine::day_of_pub(), &engine::hour_of_pub())?;
    {
        let mut st = engine.state.lock().unwrap();
        st.local = m;
        st.save();
    }
    println!("baseline rebuilt");
    Ok(())
}

fn cmd_doctor() -> Result<()> {
    let config = config::Config::load();
    for (name, r) in &config.remotes {
        let ok = remote::probe_has_devin(&r.target);
        println!(
            "{name:20} {} {}",
            r.target,
            if ok { "ok" } else { "unreachable/no db" }
        );
    }
    if config.tailscale.enabled {
        let peers = tailscale::peers();
        println!("\ntailscale peers:");
        for p in peers {
            println!(
                "  {:24} {:50} {}",
                p.hostname,
                p.dns,
                if p.online { "online" } else { "offline" }
            );
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn cmd_app(cli: &Cli) -> Result<()> {
    let _ = config::Config::write_default();
    let engine = Arc::new(engine::Engine::new(db_path(cli)));
    ui::run(engine);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn cmd_app(_cli: &Cli) -> Result<()> {
    anyhow::bail!("the menu bar app is macOS-only. On Linux/other hosts this binary runs as a remote agent: `devin-usage agent <rows|baseline|meta>`")
}
