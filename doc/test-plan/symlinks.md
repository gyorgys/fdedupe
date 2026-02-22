# Symlink Test Plan

> **Platform note**: symlink creation in `mktest` uses `std::os::unix::fs::symlink` and is only available on Unix (macOS, Linux). These tests are skipped on Windows.

## Test data

`cargo run --bin mktest` creates the following symlink:

```
testdata/gamma/alpha_link -> ../alpha
```

`gamma/alpha_link` is a directory symlink that resolves to the canonical path of `testdata/alpha/`. `testdata/alpha/` contains `hello.txt` and `unique_a.txt`, plus `nested/` with `hello_copy.txt` and `unique_b.txt`.

---

## TC-SYM-01 — Symlink not followed by default

```bash
cargo run --bin mktest
cargo run -- --db testdata/fdedupe.db scan testdata --recursive
```

**Expected**:
- `gamma/alpha_link` is encountered during enumeration of `testdata/gamma/` but is **not traversed** (no `--follow-symlinks`).
- The DB contains exactly the same 3 duplicate groups as the baseline (TC-SCAN-02 in basics.md):
  - `"hello world\n"` — 3 files
  - `"roses are red\n"` — 2 files
  - 128 KB `0xAB` block — 2 files
- No entry for `gamma/alpha_link/` appears in the `directories` table.

---

## TC-SYM-02 — Symlink followed; canonical path deduplication prevents double-counting

```bash
cargo run --bin mktest
cargo run -- --db testdata/fdedupe.db scan testdata --recursive --follow-symlinks
```

**Expected**:
- The scanner follows `gamma/alpha_link` and resolves it to the canonical path of `testdata/alpha/`.
- Because `testdata/alpha/` is already in the BFS queue (or has already been scanned), canonical-path deduplication prevents it from being processed a second time.
- Duplicate groups are **identical** to TC-SYM-01 — no double-counting of `hello.txt` etc.
- `gamma/alpha_link/` does **not** appear as a separate directory in the DB.

---

## TC-SYM-03 — Symlink followed when target scanned first

```bash
cargo run --bin mktest
# Scan alpha/ in isolation first so it is marked as scanned
cargo run -- --db testdata/fdedupe.db scan testdata/alpha --recursive
# Now scan the full tree, which encounters the symlink after the target is already in the DB
cargo run -- --db testdata/fdedupe.db scan testdata --recursive --follow-symlinks
```

**Expected**:
- When the scanner reaches `gamma/alpha_link`, the canonical target (`testdata/alpha/`) already has `last_scanned IS NOT NULL` in the DB.
- The symlinked directory is skipped (same skip logic as any already-scanned directory).
- No files from `testdata/alpha/` are double-indexed.

---

## TC-SYM-04 — Rescan with symlink followed

```bash
cargo run --bin mktest
cargo run -- --db testdata/fdedupe.db scan testdata --recursive --follow-symlinks --rescan
```

**Expected**:
- `--rescan` forces re-processing of every directory including symlink targets.
- Even if `testdata/alpha/` is encountered both directly and via `gamma/alpha_link`, canonical-path deduplication ensures it is processed only once per BFS pass.
- Duplicate groups remain the same 3 groups (no spurious duplicates introduced).

---

## TC-SYM-05 — Hidden symlink target

```bash
cargo run --bin mktest
cargo run -- --db testdata/fdedupe.db scan testdata --recursive --follow-symlinks --hidden
```

**Expected**:
- Same as TC-SYM-02, plus `hidden/.hidden_dup.txt` is included.
- `"hello world\n"` group becomes 4 files.
- No double-counting from the symlink.

---

## Failure modes to watch for

| Symptom | Likely cause |
|---------|-------------|
| `"hello world\n"` group shows 5+ files after `--follow-symlinks` | Canonical path not being resolved before DB lookup; alpha/ traversed twice |
| Infinite loop or very long scan | Circular symlink not detected; canonical path check not applied before enqueue |
| `gamma/alpha_link` appearing as its own directory row in the DB | Symlink target not being resolved to canonical path before upsert |
| Scanner crashes or errors on the symlink | Symlink not handled in directory enumeration |
