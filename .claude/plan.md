# Implementation Plan: fdedupe

## Context

`fdedupe` is a Rust CLI utility to find and remove duplicate files across multiple directories. The spec defines three modes: **scan** (find duplicates, store in DB), **list** (query and display duplicates), and **remove** (delete duplicates with user confirmation or priority rules). The user prefers **SQLite** as the database backend.

The project has no source code yet — only a spec (`doc/fdedupe.md`) and a `.gitignore` configured for Rust/Cargo.

---

## Project Setup

### Cargo.toml (create at repo root)

Key dependencies:
- `clap` v4 (derive) — CLI argument parsing
- `rusqlite` (bundled) — SQLite, self-contained binary
- `walkdir` — recursive directory traversal
- `globset` — include/exclude glob pattern matching
- `serde` + `serde_yaml` — fdedupe_options YAML config
- `blake3` — fast file hashing (both fast/partial and full)
- `ratatui` + `crossterm` — TUI for scan, list, and remove modes
- `anyhow` — error handling
- `chrono` — timestamp handling

---

## Module Structure

```
src/
├── main.rs       - Entry point: parse CLI, load config, open DB, dispatch
├── cli.rs        - Clap structs: Cli, ScanArgs, ListArgs, RemoveArgs
├── config.rs     - fdedupe_options YAML schema + loader
├── db.rs         - DB connection, schema init, all queries
├── hash.rs       - fast_hash (first 64KB) and full_hash (entire file) via blake3
├── scan.rs       - Scan mode logic
├── list.rs       - List mode (non-interactive output)
├── tui.rs        - Shared TUI infrastructure (ratatui + crossterm setup, helpers)
├── scan_tui.rs   - TUI for scan mode (progress, current dir, file counts)
├── list_tui.rs   - Interactive TUI for list mode (directory browser)
└── remove.rs     - Remove mode (TUI prompts for duplicate resolution)
```

---

## SQLite Schema

```sql
CREATE TABLE directories (
    id            INTEGER PRIMARY KEY,
    canonical_path TEXT NOT NULL UNIQUE,
    last_scanned  INTEGER  -- Unix timestamp, NULL = never completed
);

CREATE TABLE files (
    id            INTEGER PRIMARY KEY,
    directory_id  INTEGER NOT NULL REFERENCES directories(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    canonical_path TEXT NOT NULL UNIQUE,
    size          INTEGER NOT NULL,
    modified_at   INTEGER NOT NULL,  -- Unix timestamp (secs)
    fast_hash     TEXT,              -- blake3 hex of first 64KB, NULL until computed
    full_hash     TEXT,              -- blake3 hex of entire file, NULL until computed
    UNIQUE(directory_id, name)
);

CREATE TABLE rules (
    id       INTEGER PRIMARY KEY,
    pattern  TEXT NOT NULL,   -- glob pattern matched against canonical path
    priority INTEGER NOT NULL DEFAULT 0  -- higher priority = keep this file
);
```

**Database location**: `fdedupe.db` in the current working directory (configurable via YAML options or `--db` CLI flag).

---

## CLI Design

```
fdedupe [--db <path>] <COMMAND>

Commands:
  scan   [dirs...]  [--recursive] [--rescan] [--follow-symlinks]
                    [--hidden] [--include <glob>] [--exclude <glob>]
  list   [dir]      [--recursive] [--follow-symlinks] [--interactive]
  remove            [--dry-run]
```

---

## Scan Mode (`scan.rs`)

Algorithm (BFS queue over directories):

1. Resolve each input dir to canonical path
2. For each directory in queue:
   a. Check DB: if `last_scanned IS NOT NULL` and `!rescan` → skip (add subdirs to queue still if recursive)
   b. Enumerate FS entries (apply hidden filter; apply include/exclude globs to files)
   c. Load existing DB entries for this directory
   d. **Deletion detection**: items in DB not found in FS → `DELETE FROM files WHERE id = ?` (skip hidden files if hidden option off)
   e. For each FS file:
      - If DB row exists with same `size` and `modified_at` → unchanged, leave full_hash intact
      - Else: compute `fast_hash`, upsert row (clear `full_hash` if changed)
   f. After processing all files in directory: find size-collision candidates
      - `SELECT size, fast_hash, COUNT(*) FROM files GROUP BY size, fast_hash HAVING COUNT(*) > 1`
      - For candidate files missing `full_hash` → compute and store `full_hash`
   g. Set `directories.last_scanned = now()`
   h. If recursive: enqueue subdirs (follow symlinks only if `--follow-symlinks`)
3. Progress display via TUI (`scan_tui.rs`): live-updating panel showing current directory, files scanned, duplicates found so far, hashing progress. `indicatif` is no longer needed — `ratatui` handles all rendering.

---

## List Mode (`list.rs` + `list_tui.rs`)

**Non-interactive output** for a given directory:
```
Canonical path: /real/path/to/dir
Duplicates: 42 files, 1.2 GB

Subdirectories with duplicates:
  subdir_a/    15 files, 400 MB
  subdir_b/     8 files, 200 MB

Duplicate files in this directory:
  photo.jpg    4.2 MB
  video.mp4   800 MB
```

Duplicate detection query: `SELECT full_hash FROM files WHERE full_hash IS NOT NULL GROUP BY full_hash HAVING COUNT(*) > 1`

**Interactive TUI** (`list_tui.rs`):
- Uses `ratatui` with `crossterm` backend
- State: current directory (starts at input dir, cannot go above it), scroll offset, selected index
- Display: header (canonical path, duplicate count/size), scrollable list of entries (subdirs first, then files), entries with duplicates highlighted
- Keys:
  - `↑`/`↓`, `PgUp`/`PgDn` — scroll/select
  - `→`, `Enter`, `Space` — navigate into selected subdir
  - `←`, `Backspace` — go to parent (stops at root input dir)
  - `q` / `Esc` — quit

---

## Remove Mode (`remove.rs`)

Uses TUI for all interaction (ratatui fullscreen).

1. Query all duplicate groups: `SELECT full_hash, GROUP_CONCAT(canonical_path) FROM files WHERE full_hash IS NOT NULL GROUP BY full_hash HAVING COUNT(*) > 1`
2. TUI layout: header (group N of M), list of duplicate paths with selection highlight, footer showing key hints
3. For each group:
   a. Apply priority rules (from `rules` table): score each file by highest matching rule priority; if unambiguous winner → auto-keep, delete others without prompting
   b. Otherwise: show TUI with all copies listed; keys:
      - `↑`/`↓` — move selection
      - `Enter`/`d` — mark selected file to **delete** (others are kept)
      - `k` — mark selected file to **keep** (others are deleted)
      - `r` — add a priority rule: enter glob pattern + priority inline in TUI
      - `s` — skip this group
      - `q` — quit remove mode
   c. Rules entered via `r` are stored in `rules` table immediately
   d. Confirmed deletions: remove from FS, delete DB rows
4. `--dry-run`: TUI shows what would be deleted, confirm action does nothing

---

## Config File (`config.rs`)

`fdedupe_options.yaml` schema:
```yaml
db: ./fdedupe.db
recursive: false
rescan: false
follow_symlinks: false
hidden: false
include: []
exclude: []
```

Loader: check `./fdedupe_options.yaml` first, then `<exe_dir>/fdedupe_options.yaml`. CLI flags override config values.

---

## File Hashing (`hash.rs`)

- `fast_hash(path)`: open file, read up to 64 KB, return `blake3::hash(bytes)` as hex string
- `full_hash(path)`: stream entire file through `blake3::Hasher`, return hex string
- Both return `anyhow::Result<String>`

---

## Test Utility (`src/bin/mktest.rs`)

A Cargo binary (`cargo run --bin mktest`) that creates self-contained, deterministic test data under `testdata/` at the repo root. The directory is added to `.gitignore`.

Running `mktest` is idempotent: it wipes and recreates `testdata/` from scratch each time.

### Test data layout

```
testdata/
├── alpha/
│   ├── hello.txt          ("hello world\n")          <- duplicate x3
│   ├── unique_a.txt       ("unique content alpha\n") <- unique
│   └── nested/
│       ├── hello_copy.txt ("hello world\n")          <- duplicate x3
│       └── unique_b.txt   ("unique content beta\n")  <- unique
├── beta/
│   ├── hello_again.txt    ("hello world\n")          <- duplicate x3
│   ├── unique_c.txt       ("unique content gamma\n") <- unique
│   └── subdir/
│       ├── poem.txt       ("roses are red\n")        <- duplicate x2
│       └── unique_d.txt   ("unique content delta\n") <- unique
├── gamma/
│   └── poem_copy.txt      ("roses are red\n")        <- duplicate x2
├── large/
│   ├── big.bin            (128 KB, repeating 0xAB)   <- duplicate x2
│   └── big_copy.bin       (128 KB, repeating 0xAB)   <- duplicate x2
└── hidden/
    ├── .hidden_dup.txt    ("hello world\n")          <- duplicate x3 (hidden)
    └── visible.txt        ("visible only\n")         <- unique
```

### Predictable expected results (used in verification)

| Duplicate group | Files | Size each |
|----------------|-------|-----------|
| "hello world\n" | 3 (or 4 with hidden) | 12 B |
| "roses are red\n" | 2 | 15 B |
| 128 KB block | 2 | 131072 B |

`mktest` also prints a summary of what it created so the expected results are machine-readable for scripted checks.

---

## Critical Files to Create

| File | Purpose |
|------|---------|
| `Cargo.toml` | Project manifest and dependencies |
| `src/main.rs` | CLI dispatch |
| `src/cli.rs` | Clap arg structs |
| `src/config.rs` | YAML config |
| `src/db.rs` | All DB operations |
| `src/hash.rs` | blake3 hashing |
| `src/scan.rs` | Scan mode |
| `src/list.rs` | Non-interactive list output |
| `src/tui.rs` | Shared TUI setup/helpers |
| `src/scan_tui.rs` | Scan progress TUI |
| `src/list_tui.rs` | Interactive list TUI (directory browser) |
| `src/remove.rs` | Remove mode with TUI prompts |
| `src/bin/mktest.rs` | Test data generator |

`.gitignore` additions: `testdata/`, `fdedupe.db`

---

## Verification

All steps use `testdata/` created by `cargo run --bin mktest`.
DB is always isolated to `testdata/fdedupe.db` via `--db` so it never pollutes the repo root.

```
DB=testdata/fdedupe.db
```

1. `cargo build` — must compile cleanly
2. Generate test data: `cargo run --bin mktest` (also prints ready-to-use commands)
3. Scan (non-recursive): `cargo run -- --db $DB scan testdata/alpha` → only top-level files indexed
4. Scan (recursive): `cargo run -- --db $DB scan testdata --recursive` → all files; 3 duplicate groups
5. List: `cargo run -- --db $DB list testdata` → shows 3 groups with correct file counts and sizes
6. List recursive: `cargo run -- --db $DB list testdata --recursive` → each subdir shows local duplicates
7. Interactive list: `cargo run -- --db $DB list testdata --interactive` → TUI launches, arrow keys navigate
8. Remove dry-run: `cargo run -- --db $DB remove --dry-run` → shows 3 duplicate groups, nothing deleted
9. Re-scan (skip): run scan again without `--rescan` → already-scanned directories are skipped
10. Re-scan (forced): `cargo run -- --db $DB scan testdata --recursive --rescan` → all dirs re-processed
11. Hidden files: `cargo run -- --db $DB scan testdata --recursive --hidden` → "hello world" group becomes 4 files
12. Config file: `fdedupe_options.yaml` with `recursive: true` in CWD, `cargo run -- --db $DB scan testdata` → behaves as recursive
