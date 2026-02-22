mod cli;
mod config;
mod db;
mod hash;
mod list;
mod list_tui;
mod remove;
mod scan;
mod scan_tui;
mod tui;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

use cli::{Cli, Command};
use config::Config;
use db::Db;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load()?;

    let db_path: PathBuf = cli
        .db
        .clone()
        .or_else(|| config.db.clone())
        .unwrap_or_else(|| PathBuf::from("fdedupe.db"));

    let db = Db::open(&db_path)?;

    match &cli.command {
        Command::Scan(args) => scan::run(args, &config, &db)?,
        Command::List(args) => list::run(args, &config, &db)?,
        Command::Remove(args) => remove::run(args, &config, &db)?,
    }

    Ok(())
}
