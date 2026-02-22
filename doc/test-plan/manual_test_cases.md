# Manual Test Cases

These tests require an interactive terminal (TTY) and cannot be run in non-TTY environments such as piped shells or VS Code's embedded terminal. Run them in a regular terminal emulator.

All test runs use `--db testdata/fdedupe.db`. Run `cargo run --bin mktest` followed by `cargo run -- --db testdata/fdedupe.db scan testdata --recursive` to populate the DB before starting.

---

## TC-LIST-03 — Interactive list TUI

```bash
cargo run -- --db testdata/fdedupe.db list testdata --interactive
```

**Expected**:
- TUI launches in fullscreen.
- Header shows the canonical path of `testdata/` and total duplicate count/size.
- List shows subdirectories (yellow if they contain duplicates) and files in the current directory.
- `↑`/`↓` moves the selection; `PgUp`/`PgDn` scrolls by page.
- `→`, `Enter`, or `Space` on a subdirectory navigates into it; the header updates.
- `←` or `Backspace` navigates to the parent; stops at `testdata/` (cannot go above the root).
- `q` or `Esc` exits cleanly, restoring the terminal.

---

## TC-REMOVE-01 — Remove dry run

```bash
cargo run -- --db testdata/fdedupe.db remove --dry-run
```

**Expected**:
- TUI launches showing the first duplicate group.
- All 3 groups are browsable (one at a time).
- Confirming a deletion in dry-run mode shows what would be deleted but does **not** delete files or modify the DB.
- `q` exits; files on disk and DB rows are unchanged.

---

## TC-REMOVE-02 — Interactive removal

```bash
cargo run -- --db testdata/fdedupe.db remove
```

**Expected**:
- TUI shows each duplicate group in turn (header: "Group N of M").
- `↑`/`↓` moves selection within the group's file list.
- `k` marks the selected file to keep (others will be deleted).
- `d` or `Enter` marks the selected file to delete.
- `s` skips the current group without action.
- `r` prompts for a glob pattern and priority to add a rule; the rule is saved to the DB immediately and applied to subsequent groups.
- `q` quits; any groups already confirmed are already deleted.
- Confirmed deletions: files are removed from disk and their rows are removed from the DB.

---

## TC-REMOVE-03 — Priority rule auto-resolution

Add a rule that unambiguously favours one file from a duplicate group, then run remove:

```bash
# Insert a rule that gives higher priority to files under alpha/
sqlite3 testdata/fdedupe.db "INSERT INTO rules (pattern, priority) VALUES ('**/alpha/**', 10)"
cargo run -- --db testdata/fdedupe.db remove
```

**Expected**:
- The `"hello world\n"` group is auto-resolved without prompting: the file under `alpha/` is kept, the others are deleted.
- Groups with no matching rule still prompt interactively.
