# Change Detection Test Plan

Tests verify that scan correctly detects file modifications, additions, and deletions when directories are rescanned. All tests use `--db testdata/fdedupe.db`.

> **Setup for each test**: unless otherwise noted, start from a fresh state:
> ```bash
> cargo run --bin mktest
> cargo run -- --db testdata/fdedupe.db scan testdata --recursive
> ```

---

## File modification

### TC-CHG-01 — Modified file detected and re-hashed on rescan

Modify `alpha/hello.txt` so it no longer matches `"hello world\n"`, then rescan with `--rescan`.

```bash
cargo run --bin mktest
cargo run -- --db testdata/fdedupe.db scan testdata --recursive
printf 'something else\n' > testdata/alpha/hello.txt
cargo run -- --db testdata/fdedupe.db scan testdata/alpha --rescan
```

**Expected**:
- `alpha/hello.txt` has a new `fast_hash` and `full_hash` in the DB.
- The `"hello world\n"` duplicate group shrinks from 3 files to 2 (`alpha/nested/hello_copy.txt` and `beta/hello_again.txt`).
- `alpha/hello.txt` is no longer part of any duplicate group (its new content is unique).

```bash
# Verify: hello world group now has 2 files
sqlite3 testdata/fdedupe.db \
  "SELECT COUNT(*) FROM files WHERE full_hash = (SELECT full_hash FROM files WHERE name='hello_copy.txt')"
# Expected: 2
```

---

### TC-CHG-02 — Modified file NOT detected when directory is skipped (no --rescan)

Modify `alpha/hello.txt`, then scan the same directory without `--rescan`. Because `alpha/` was already marked as scanned, it is skipped entirely.

```bash
cargo run --bin mktest
cargo run -- --db testdata/fdedupe.db scan testdata --recursive
printf 'something else\n' > testdata/alpha/hello.txt
cargo run -- --db testdata/fdedupe.db scan testdata/alpha
```

**Expected**:
- Scan output shows 0 files scanned (directory skipped).
- `alpha/hello.txt` still has the old hash in the DB.
- `"hello world\n"` group still has 3 files.

This is correct behaviour: the skip-if-scanned optimisation avoids redundant work. Use `--rescan` to force re-evaluation.

---

### TC-CHG-03 — Same-size modification detected via `modified_at`

Replace `alpha/hello.txt` with content of the same byte length but different bytes. The filesystem `mtime` will differ, so scan still detects the change.

```bash
cargo run --bin mktest
cargo run -- --db testdata/fdedupe.db scan testdata --recursive
# "hello world\n" is 12 bytes; write a different 12-byte string
printf 'goodbye now\n' > testdata/alpha/hello.txt
cargo run -- --db testdata/fdedupe.db scan testdata/alpha --rescan
```

**Expected**:
- `alpha/hello.txt` is re-hashed (size unchanged, but `modified_at` changed).
- Its new `full_hash` does not match any other file.
- `"hello world\n"` group shrinks from 3 to 2.

---

## File deletion

### TC-CHG-04 — Deleted file removed from DB on rescan

```bash
cargo run --bin mktest
cargo run -- --db testdata/fdedupe.db scan testdata --recursive
rm testdata/alpha/hello.txt
cargo run -- --db testdata/fdedupe.db scan testdata/alpha --rescan
```

**Expected**:
- Scan reports 1 file deleted.
- `alpha/hello.txt` row is absent from the `files` table.
- `"hello world\n"` group shrinks from 3 to 2 files.

```bash
sqlite3 testdata/fdedupe.db "SELECT COUNT(*) FROM files WHERE name='hello.txt' AND canonical_path LIKE '%alpha/hello.txt'"
# Expected: 0
```

---

### TC-CHG-05 — Deleted file NOT detected when directory is skipped (no --rescan)

```bash
cargo run --bin mktest
cargo run -- --db testdata/fdedupe.db scan testdata --recursive
rm testdata/alpha/hello.txt
cargo run -- --db testdata/fdedupe.db scan testdata/alpha
```

**Expected**:
- Directory skipped (already scanned), 0 files processed.
- `alpha/hello.txt` row still present in DB.
- `"hello world\n"` group still shows 3 files.

---

### TC-CHG-06 — Hidden file deletion skipped when not scanning hidden files

This verifies the explicit guard: when `--hidden` is not set, deletion detection skips hidden DB entries, so a deleted hidden file is not removed from the DB.

```bash
cargo run --bin mktest
# Scan with --hidden so .hidden_dup.txt enters the DB
cargo run -- --db testdata/fdedupe.db scan testdata --recursive --hidden
rm testdata/hidden/.hidden_dup.txt
# Rescan without --hidden
cargo run -- --db testdata/fdedupe.db scan testdata/hidden --rescan
```

**Expected**:
- `.hidden_dup.txt` is NOT removed from the DB (hidden deletion detection is suppressed).
- The `"hello world\n"` group still shows 4 files in the DB.

Now rescan with `--hidden` to confirm it IS removed when the flag is present:

```bash
cargo run -- --db testdata/fdedupe.db scan testdata/hidden --rescan --hidden
```

**Expected**:
- `.hidden_dup.txt` row is deleted from the DB.
- `"hello world\n"` group shrinks back to 3 files.

---

## File addition

### TC-CHG-07 — New file not detected in already-scanned directory (no --rescan)

```bash
cargo run --bin mktest
cargo run -- --db testdata/fdedupe.db scan testdata --recursive
printf 'hello world\n' > testdata/alpha/new_dup.txt
cargo run -- --db testdata/fdedupe.db scan testdata/alpha
```

**Expected**:
- Directory skipped, 0 files processed.
- `new_dup.txt` is absent from the DB.
- `"hello world\n"` group still has 3 files.

---

### TC-CHG-08 — New file detected on rescan and joins duplicate group

```bash
cargo run --bin mktest
cargo run -- --db testdata/fdedupe.db scan testdata --recursive
printf 'hello world\n' > testdata/alpha/new_dup.txt
cargo run -- --db testdata/fdedupe.db scan testdata/alpha --rescan
```

**Expected**:
- `new_dup.txt` is indexed with a `full_hash` matching `"hello world\n"`.
- `"hello world\n"` group grows from 3 to 4 files.

```bash
sqlite3 testdata/fdedupe.db \
  "SELECT COUNT(*) FROM files WHERE full_hash = (SELECT full_hash FROM files WHERE name='hello_copy.txt')"
# Expected: 4
```

---

## Directory deletion

### TC-CHG-09 — Deleted directory rows remain in DB (known limitation)

The scan algorithm detects deleted **files** within a directory it processes, but it does not walk the DB to find directories that no longer exist on disk. If a directory is deleted, its row (and its files) remain in the DB until they are explicitly removed.

```bash
cargo run --bin mktest
cargo run -- --db testdata/fdedupe.db scan testdata --recursive
rm -rf testdata/alpha/nested/
# Rescan alpha/ and its tree
cargo run -- --db testdata/fdedupe.db scan testdata/alpha --recursive --rescan
```

**Expected (current behaviour)**:
- `alpha/` is processed: its own files are checked, `nested/` is not found on disk so it is not enqueued.
- The `directories` row for `alpha/nested/` and its two file rows (`hello_copy.txt`, `unique_b.txt`) **remain in the DB**.
- Duplicate counts may be inflated: `"hello world\n"` still shows 3 files even though one copy no longer exists.

**Desired future behaviour**: a full-tree rescan should detect and remove directory rows whose paths no longer exist, then cascade-delete their files.

```bash
# Confirm the stale rows are still present
sqlite3 testdata/fdedupe.db "SELECT canonical_path FROM directories WHERE canonical_path LIKE '%nested%'"
# Currently returns the nested/ row; a future fix would return nothing
```

---

## Summary of expected scan behaviour

| Scenario | Detects change? | Condition |
|----------|-----------------|-----------|
| File modified (size or mtime changed) | Yes | Directory rescanned (`--rescan` or first scan) |
| File modified, directory skipped | No | Directory already marked scanned, no `--rescan` |
| File deleted | Yes | Directory rescanned |
| File deleted, directory skipped | No | Directory already marked scanned, no `--rescan` |
| Hidden file deleted, hidden=false | No | Deletion detection skips hidden entries |
| Hidden file deleted, hidden=true | Yes | Deletion detection includes hidden entries |
| New file added | Yes | Directory rescanned |
| New file added, directory skipped | No | Directory already marked scanned, no `--rescan` |
| Directory deleted | No (limitation) | No directory-level deletion walk implemented |
