# fdedupe — Architecture

## Module Structure

```
src/
├── main.rs       Entry point: parse CLI, load config, open DB, dispatch
├── cli.rs        Clap structs: Cli, ScanArgs, ListArgs, RemoveArgs
├── config.rs     fdedupe_options YAML schema + loader
├── db.rs         DB connection, schema init, all queries
├── hash.rs       fast_hash (first 64 KB) and full_hash (entire file) via blake3
├── scan.rs       Scan mode logic
├── list.rs       Non-interactive list output
├── tui.rs        Shared TUI helpers (enter/leave terminal, key polling, fmt_size)
├── scan_tui.rs   Live scan progress TUI (falls back to plain stderr when not a TTY)
├── list_tui.rs   Interactive directory browser TUI
├── remove.rs     Remove mode with TUI prompts and priority rules
└── bin/
    └── mktest.rs Test data generator
```

## SQLite Schema

```sql
CREATE TABLE directories (
    id             INTEGER PRIMARY KEY,
    canonical_path TEXT    NOT NULL UNIQUE,
    last_scanned   INTEGER           -- Unix timestamp, NULL = never completed
);

CREATE TABLE files (
    id             INTEGER PRIMARY KEY,
    directory_id   INTEGER NOT NULL REFERENCES directories(id) ON DELETE CASCADE,
    name           TEXT    NOT NULL,
    canonical_path TEXT    NOT NULL UNIQUE,
    size           INTEGER NOT NULL,
    modified_at    INTEGER NOT NULL, -- Unix timestamp (secs)
    fast_hash      TEXT,             -- blake3 hex of first 64 KB, NULL until computed
    full_hash      TEXT,             -- blake3 hex of entire file, NULL until computed
    UNIQUE(directory_id, name)
);

CREATE TABLE rules (
    id       INTEGER PRIMARY KEY,
    pattern  TEXT    NOT NULL,       -- glob pattern matched against canonical path
    priority INTEGER NOT NULL DEFAULT 0  -- higher = keep this file
);
```

Database location: `fdedupe.db` in the current working directory, overridden by `fdedupe_options.yaml` or the `--db` CLI flag.

## CLI

```
fdedupe [--db <path>] <COMMAND>

Commands:
  scan   [dirs...]  [--recursive] [--rescan] [--follow-symlinks]
                    [--hidden] [--include <glob>] [--exclude <glob>]
  list   [dir]      [--recursive] [--follow-symlinks] [--interactive]
  remove            [--dry-run]
```

## Scan Algorithm

BFS queue over directories:

1. Resolve each input dir to its canonical path.
2. For each directory in the queue:
   a. If `last_scanned IS NOT NULL` and `--rescan` not set → skip (still enqueue subdirs if recursive).
   b. Enumerate FS entries; apply hidden filter and include/exclude globs to files.
   c. Load existing DB entries for this directory.
   d. **Deletion detection**: DB rows not found on FS → `DELETE FROM files`. Hidden files are skipped during deletion detection when the hidden option is off.
   e. For each FS file:
      - Same `size` and `modified_at` as DB row → unchanged; leave `full_hash` intact.
      - New or changed → compute `fast_hash`, upsert row, clear `full_hash`.
   f. Find size+fast_hash collision candidates; compute `full_hash` for any that are missing it.
   g. Set `directories.last_scanned = now()`.
   h. If recursive: enqueue subdirs (follow symlinks only if `--follow-symlinks`).
3. Progress shown via `scan_tui::ScanProgress`; falls back to plain stderr when not running in a TTY.

## File Hashing

Two-phase strategy via blake3:

- **fast_hash**: read up to 64 KB, hash that buffer. Used to quickly filter out non-duplicates before reading entire files.
- **full_hash**: stream the entire file through `blake3::Hasher`. Computed only when two or more files share the same `size` and `fast_hash`.

## Config File

`fdedupe_options.yaml` loaded from CWD first, then the executable's directory. CLI flags override all config values.

```yaml
db: ./fdedupe.db
recursive: false
rescan: false
follow_symlinks: false
hidden: false
include: []
exclude: []
```

## Remove Mode

1. Query all duplicate groups (files sharing a `full_hash`).
2. Apply priority rules: score each file by the highest-priority rule whose glob matches its canonical path. If one file scores highest unambiguously → auto-keep it, delete the rest without prompting.
3. Otherwise: show a TUI listing all copies.
   - `↑`/`↓` — move selection
   - `k` — mark selected file to keep (others will be deleted)
   - `d` / `Enter` — mark selected file to delete (others are kept)
   - `r` — add a priority rule inline (glob + priority, persisted to `rules` table immediately)
   - `s` — skip this group
   - `q` — quit remove mode
4. `--dry-run`: show what would be deleted; confirmed action does nothing.
