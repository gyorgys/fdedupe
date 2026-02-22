# Basic Test Plan

All test runs use `--db testdata/fdedupe.db` to keep the database isolated from the repo root.

## Setup

```bash
cargo build
cargo run --bin mktest      # wipe and recreate testdata/ with known duplicates
```

Expected test data layout after `mktest`:

```
testdata/
├── alpha/
│   ├── hello.txt              "hello world\n"          — duplicate ×3
│   ├── unique_a.txt           "unique content alpha\n" — unique
│   └── nested/
│       ├── hello_copy.txt     "hello world\n"          — duplicate ×3
│       └── unique_b.txt       "unique content beta\n"  — unique
├── beta/
│   ├── hello_again.txt        "hello world\n"          — duplicate ×3
│   ├── unique_c.txt           "unique content gamma\n" — unique
│   └── subdir/
│       ├── poem.txt           "roses are red\n"        — duplicate ×2
│       └── unique_d.txt       "unique content delta\n" — unique
├── gamma/
│   ├── poem_copy.txt          "roses are red\n"        — duplicate ×2
│   └── alpha_link -> ../alpha (symlink, Unix only)
├── large/
│   ├── big.bin                128 KB 0xAB block        — duplicate ×2
│   └── big_copy.bin           128 KB 0xAB block        — duplicate ×2
└── hidden/
    ├── .hidden_dup.txt        "hello world\n"          — duplicate (hidden)
    └── visible.txt            "visible only\n"         — unique
```

---

## Scan

### TC-SCAN-01 — Non-recursive scan (single directory)

```bash
cargo run -- --db testdata/fdedupe.db scan testdata/alpha
```

**Expected**: only the two top-level files in `testdata/alpha/` are indexed (`hello.txt`, `unique_a.txt`). `nested/` is not traversed.

---

### TC-SCAN-02 — Full recursive scan

```bash
cargo run -- --db testdata/fdedupe.db scan testdata --recursive
```

**Expected**: all directories traversed; 3 duplicate groups recorded.

| Duplicate group     | Files | Size each |
|---------------------|-------|-----------|
| `"hello world\n"`   | 3     | 12 B      |
| `"roses are red\n"` | 2     | 15 B      |
| 128 KB `0xAB` block | 2     | 131072 B  |

---

### TC-SCAN-03 — Skip already-scanned directories

Run TC-SCAN-02 a second time **without** `--rescan`:

```bash
cargo run -- --db testdata/fdedupe.db scan testdata --recursive
```

**Expected**: all directories report as already scanned and are skipped. Duplicate groups in the DB are unchanged.

---

### TC-SCAN-04 — Force rescan

```bash
cargo run -- --db testdata/fdedupe.db scan testdata --recursive --rescan
```

**Expected**: all directories are re-processed regardless of `last_scanned`. Duplicate groups remain the same 3 groups.

---

### TC-SCAN-05 — Include hidden files

```bash
cargo run -- --db testdata/fdedupe.db scan testdata --recursive --hidden
```

**Expected**: `hidden/.hidden_dup.txt` is included. The `"hello world\n"` group grows from 3 to 4 files.

---

## List

Run TC-SCAN-02 first to populate the DB.

### TC-LIST-01 — Non-interactive summary

```bash
cargo run -- --db testdata/fdedupe.db list testdata
```

**Expected**: prints canonical path, total duplicate count and size, subdirectory breakdown, and duplicate files directly in `testdata/` (none — all duplicates are in subdirs).

---

### TC-LIST-02 — Recursive list

```bash
cargo run -- --db testdata/fdedupe.db list testdata --recursive
```

**Expected**: same output as TC-LIST-01 followed by the same summary for each subdirectory that contains duplicates.

---

> TC-LIST-03 (interactive TUI) requires a real terminal — see [manual_test_cases.md](manual_test_cases.md).

---

## Remove

> All remove test cases require a real terminal (TUI) — see [manual_test_cases.md](manual_test_cases.md).

---

## Config file override

### TC-CONFIG-01 — `recursive` from config

Create `fdedupe_options.yaml` in the working directory:

```yaml
recursive: true
```

Then run:

```bash
cargo run -- --db testdata/fdedupe.db scan testdata
```

**Expected**: behaves as if `--recursive` was passed; all directories are traversed.
