use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::cli::ScanArgs;
use crate::config::Config;
use crate::db::Db;
use crate::hash;
use crate::scan_tui::ScanProgress;

pub struct ScanOptions {
    pub recursive: bool,
    pub rescan: bool,
    pub follow_symlinks: bool,
    pub hidden: bool,
    pub include: GlobSet,
    pub exclude: GlobSet,
}

impl ScanOptions {
    pub fn from_args_and_config(args: &ScanArgs, config: &Config) -> Result<Self> {
        let recursive = args.recursive || config.recursive;
        let rescan = args.rescan || config.rescan;
        let follow_symlinks = args.follow_symlinks || config.follow_symlinks;
        let hidden = args.hidden || config.hidden;

        // CLI include/exclude take priority; fall back to config
        let include_globs: Vec<&str> = if !args.include.is_empty() {
            args.include.iter().map(|s| s.as_str()).collect()
        } else {
            config.include.iter().map(|s| s.as_str()).collect()
        };
        let exclude_globs: Vec<&str> = if !args.exclude.is_empty() {
            args.exclude.iter().map(|s| s.as_str()).collect()
        } else {
            config.exclude.iter().map(|s| s.as_str()).collect()
        };

        Ok(Self {
            recursive,
            rescan,
            follow_symlinks,
            hidden,
            include: build_globset(&include_globs)?,
            exclude: build_globset(&exclude_globs)?,
        })
    }

    fn file_included(&self, name: &str) -> bool {
        if !self.include.is_empty() && !self.include.is_match(name) {
            return false;
        }
        if self.exclude.is_match(name) {
            return false;
        }
        true
    }

    fn is_hidden(name: &str) -> bool {
        name.starts_with('.')
    }
}

pub fn run(args: &ScanArgs, config: &Config, db: &Db) -> Result<()> {
    let opts = ScanOptions::from_args_and_config(args, config)?;

    let dirs: Vec<PathBuf> = if args.dirs.is_empty() {
        vec![std::env::current_dir()?]
    } else {
        args.dirs.clone()
    };

    let mut progress = ScanProgress::new();
    progress.start()?;

    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    for dir in dirs {
        match dir.canonicalize() {
            Ok(canonical) => queue.push_back(canonical),
            Err(e) => {
                progress.log(format!("Skipping {}: {}", dir.display(), e));
            }
        }
    }

    while let Some(dir_path) = queue.pop_front() {
        let dir_str = dir_path.to_string_lossy().into_owned();
        progress.set_current_dir(dir_str.clone());

        // Get or create directory record
        let dir_id = db.upsert_directory(&dir_str)?;
        let dir_row = db.get_directory(&dir_str)?.unwrap();

        // Skip if already scanned and rescan not requested
        if dir_row.last_scanned.is_some() && !opts.rescan {
            if opts.recursive {
                enqueue_subdirs(&dir_path, &opts, &mut queue);
            }
            continue;
        }

        // Enumerate filesystem entries
        let (fs_files, fs_subdirs) = enumerate_dir(&dir_path, &opts)?;

        // Load existing DB files for this directory
        let db_files = db.files_in_directory(dir_id)?;

        // Deletion detection: files in DB but not in FS
        let fs_file_names: std::collections::HashSet<&str> =
            fs_files.iter().map(|(n, _)| n.as_str()).collect();
        for db_file in &db_files {
            // If we're not scanning hidden files, skip hidden DB entries for deletion check
            if !opts.hidden && ScanOptions::is_hidden(&db_file.name) {
                continue;
            }
            if !fs_file_names.contains(db_file.name.as_str()) {
                db.delete_file(db_file.id)?;
                progress.inc_deleted();
            }
        }

        // Directory deletion detection: child dirs in DB but not on the filesystem
        let fs_subdir_set: std::collections::HashSet<String> =
            fs_subdirs.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        for child in db.child_directories(&dir_str)? {
            if !fs_subdir_set.contains(&child.canonical_path) {
                db.delete_directory_tree(&child.canonical_path)?;
                progress.log(format!("Removed deleted directory: {}", child.canonical_path));
            }
        }

        // Process each filesystem file
        let db_file_map: std::collections::HashMap<&str, &crate::db::FileRow> =
            db_files.iter().map(|f| (f.name.as_str(), f)).collect();

        for (name, full_path) in &fs_files {
            let meta = match std::fs::metadata(full_path) {
                Ok(m) => m,
                Err(e) => {
                    progress.log(format!("Cannot stat {}: {}", full_path.display(), e));
                    continue;
                }
            };
            let size = meta.len() as i64;
            let modified_at = system_time_to_secs(meta.modified().unwrap_or(SystemTime::UNIX_EPOCH));

            let full_path_str = full_path.to_string_lossy().into_owned();

            if let Some(existing) = db_file_map.get(name.as_str()) {
                if existing.size == size && existing.modified_at == modified_at {
                    // Unchanged — skip
                    progress.inc_scanned();
                    continue;
                }
                // Changed — recompute fast hash, clear full hash
                match hash::fast_hash(full_path) {
                    Ok(fh) => {
                        db.upsert_file(dir_id, name, &full_path_str, size, modified_at, Some(&fh), None)?;
                    }
                    Err(e) => {
                        progress.log(format!("fast_hash {}: {}", full_path.display(), e));
                    }
                }
            } else {
                // New file
                match hash::fast_hash(full_path) {
                    Ok(fh) => {
                        db.upsert_file(dir_id, name, &full_path_str, size, modified_at, Some(&fh), None)?;
                    }
                    Err(e) => {
                        progress.log(format!("fast_hash {}: {}", full_path.display(), e));
                    }
                }
            }
            progress.inc_scanned();
        }

        // Compute full hashes for collision candidates
        let candidates = db.candidates_needing_full_hash()?;
        for file in candidates {
            // Only process files under the current scan scope
            let path = PathBuf::from(&file.canonical_path);
            match hash::full_hash(&path) {
                Ok(fh) => {
                    db.update_full_hash(file.id, &fh)?;
                    progress.inc_hashed();
                }
                Err(e) => {
                    progress.log(format!("full_hash {}: {}", path.display(), e));
                }
            }
        }

        // Mark directory as scanned
        let now = system_time_to_secs(SystemTime::now());
        db.set_directory_scanned(dir_id, now)?;

        if opts.recursive {
            for subdir in fs_subdirs {
                queue.push_back(subdir);
            }
        }
    }

    // Final duplicate count
    let groups = db.duplicate_groups()?;
    progress.finish(groups.len())?;

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn enumerate_dir(
    dir: &Path,
    opts: &ScanOptions,
) -> Result<(Vec<(String, PathBuf)>, Vec<PathBuf>)> {
    let mut files = Vec::new();
    let mut subdirs = Vec::new();

    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => return Err(e.into()),
    };

    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();

        if !opts.hidden && ScanOptions::is_hidden(&name) {
            continue;
        }

        let file_type = if opts.follow_symlinks {
            entry.metadata().map(|m| m.file_type())
        } else {
            entry.file_type().map(|ft| ft)
        };

        let ft = match file_type {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        if ft.is_dir() {
            // Resolve to canonical to avoid following the same dir twice via symlinks
            if let Ok(canonical) = entry.path().canonicalize() {
                subdirs.push(canonical);
            }
        } else if ft.is_file() {
            if !opts.file_included(&name) {
                continue;
            }
            let canonical = if opts.follow_symlinks {
                entry.path().canonicalize().unwrap_or_else(|_| entry.path())
            } else {
                entry.path().canonicalize().unwrap_or_else(|_| entry.path())
            };
            files.push((name, canonical));
        }
        // Symlinks not followed are skipped (is_file()/is_dir() returns false for symlinks when not following)
    }

    Ok((files, subdirs))
}

fn enqueue_subdirs(dir: &Path, opts: &ScanOptions, queue: &mut VecDeque<PathBuf>) {
    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !opts.hidden && ScanOptions::is_hidden(&name) {
                continue;
            }
            let ft = if opts.follow_symlinks {
                entry.metadata().map(|m| m.file_type())
            } else {
                entry.file_type().map(|ft| ft)
            };
            if let Ok(ft) = ft {
                if ft.is_dir() {
                    if let Ok(canonical) = entry.path().canonicalize() {
                        queue.push_back(canonical);
                    }
                }
            }
        }
    }
}

fn build_globset(patterns: &[&str]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        builder.add(Glob::new(p)?);
    }
    Ok(builder.build()?)
}

fn system_time_to_secs(t: SystemTime) -> i64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
