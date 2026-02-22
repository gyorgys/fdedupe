use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fdedupe", about = "Find and remove duplicate files")]
pub struct Cli {
    /// Path to the SQLite database file
    #[arg(long, global = true)]
    pub db: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Scan directories for duplicate files
    Scan(ScanArgs),
    /// List duplicate files from the database
    List(ListArgs),
    /// Remove duplicate files interactively
    Remove(RemoveArgs),
}

#[derive(Args)]
pub struct ScanArgs {
    /// Directories to scan (default: current directory)
    pub dirs: Vec<PathBuf>,

    /// Scan subdirectories recursively
    #[arg(short, long)]
    pub recursive: bool,

    /// Re-scan directories even if already scanned
    #[arg(long)]
    pub rescan: bool,

    /// Follow symbolic links
    #[arg(long)]
    pub follow_symlinks: bool,

    /// Include hidden files and directories
    #[arg(long)]
    pub hidden: bool,

    /// Include only files matching these glob patterns
    #[arg(long, value_name = "GLOB")]
    pub include: Vec<String>,

    /// Exclude files matching these glob patterns
    #[arg(long, value_name = "GLOB")]
    pub exclude: Vec<String>,
}

#[derive(Args)]
pub struct ListArgs {
    /// Directory to list duplicates for (default: current directory)
    pub dir: Option<PathBuf>,

    /// List duplicates recursively in subdirectories
    #[arg(short, long)]
    pub recursive: bool,

    /// Follow symbolic links
    #[arg(long)]
    pub follow_symlinks: bool,

    /// Use interactive TUI browser
    #[arg(short, long)]
    pub interactive: bool,
}

#[derive(Args)]
pub struct RemoveArgs {
    /// Show what would be deleted without actually deleting
    #[arg(long)]
    pub dry_run: bool,
}
