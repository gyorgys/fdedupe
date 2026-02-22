use anyhow::Result;
use std::path::Path;

use crate::cli::ListArgs;
use crate::config::Config;
use crate::db::Db;
use crate::tui::fmt_size;

pub fn run(args: &ListArgs, _config: &Config, db: &Db) -> Result<()> {
    let dir = match &args.dir {
        Some(d) => d.canonicalize()?,
        None => std::env::current_dir()?.canonicalize()?,
    };

    if args.interactive {
        return crate::list_tui::run(&dir, db);
    }

    print_dir(&dir, args.recursive, args.follow_symlinks, db)?;
    Ok(())
}

fn print_dir(dir: &Path, recursive: bool, _follow_symlinks: bool, db: &Db) -> Result<()> {
    let dir_str = dir.to_string_lossy();
    let dir_row = db.get_directory(&dir_str)?;

    println!();
    println!("Canonical path: {}", dir_str);

    let Some(dir_row) = dir_row else {
        println!("  (not in database — run 'fdedupe scan' first)");
        return Ok(());
    };

    let (dup_count, dup_size) = db.duplicate_stats_under(&dir_str)?;
    println!(
        "Duplicates: {} files, {}",
        dup_count,
        fmt_size(dup_size)
    );

    // Child directories with duplicates
    let children = db.child_directories(&dir_str)?;
    let mut child_dups: Vec<(String, i64, i64)> = Vec::new();
    for child in &children {
        let (count, size) = db.duplicate_stats_under(&child.canonical_path)?;
        if count > 0 {
            child_dups.push((child.canonical_path.clone(), count, size));
        }
    }

    if !child_dups.is_empty() {
        println!();
        println!("Subdirectories with duplicates:");
        for (path, count, size) in &child_dups {
            let rel = relative_name(dir, path);
            println!("  {:40}  {} files, {}", rel + "/", count, fmt_size(*size));
        }
    }

    // Duplicate files directly in this directory
    let dup_files = db.duplicate_files_in_dir(dir_row.id)?;
    if !dup_files.is_empty() {
        println!();
        println!("Duplicate files in this directory:");
        for f in &dup_files {
            println!("  {:40}  {}", f.name, fmt_size(f.size));
        }
    }

    if recursive {
        for child in &children {
            let child_path = Path::new(&child.canonical_path);
            print_dir(child_path, recursive, _follow_symlinks, db)?;
        }
    }

    Ok(())
}

fn relative_name(base: &Path, child: &str) -> String {
    let base_str = base.to_string_lossy();
    let prefix = format!("{}/", base_str.trim_end_matches('/'));
    if child.starts_with(prefix.as_str()) {
        child[prefix.len()..].to_string()
    } else {
        child.to_string()
    }
}
