# fdedupe — Claude Code Instructions

## Build

```bash
cargo build
```

## Verification

Always use `--db testdata/fdedupe.db` for test runs to keep the DB isolated from the repo root.

### Setup
```bash
cargo run --bin mktest          # wipe and recreate testdata/ with known duplicates
```

### Scan
```bash
cargo run -- --db testdata/fdedupe.db scan testdata/alpha                       # non-recursive: top-level files only
cargo run -- --db testdata/fdedupe.db scan testdata --recursive                 # full scan; expect 3 duplicate groups
cargo run -- --db testdata/fdedupe.db scan testdata --recursive --rescan        # force re-scan all dirs
cargo run -- --db testdata/fdedupe.db scan testdata --recursive --hidden        # include hidden; "hello world" group → 4 files
```

### List
```bash
cargo run -- --db testdata/fdedupe.db list testdata                             # non-interactive summary
cargo run -- --db testdata/fdedupe.db list testdata --recursive                 # per-subdir breakdown
cargo run -- --db testdata/fdedupe.db list testdata --interactive               # TUI browser (arrow keys to navigate)
```

### Remove
```bash
cargo run -- --db testdata/fdedupe.db remove --dry-run                         # show groups, delete nothing
cargo run -- --db testdata/fdedupe.db remove                                   # interactive TUI removal
```

## Expected test results (after full recursive scan)

| Duplicate group       | Files | Size each  |
|-----------------------|-------|------------|
| `"hello world\n"`     | 3     | 12 B       |
| `"roses are red\n"`   | 2     | 15 B       |
| 128 KB `0xAB` block   | 2     | 131072 B   |

With `--hidden`: `"hello world\n"` group becomes 4 files (`hidden/.hidden_dup.txt` included).

## Key files

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point |
| `src/cli.rs` | CLI argument structs (clap) |
| `src/config.rs` | `fdedupe_options.yaml` loader |
| `src/db.rs` | All SQLite queries and schema |
| `src/hash.rs` | blake3 fast (64 KB) and full hashing |
| `src/scan.rs` | Scan mode logic |
| `src/tui.rs` | Shared TUI helpers (enter/leave terminal, key polling, fmt_size) |
| `src/scan_tui.rs` | Live scan progress TUI |
| `src/list.rs` | Non-interactive list output |
| `src/list_tui.rs` | Interactive directory browser TUI |
| `src/remove.rs` | Remove mode TUI |
| `src/bin/mktest.rs` | Test data generator |
| `.claude/plan.md` | Full implementation plan |
