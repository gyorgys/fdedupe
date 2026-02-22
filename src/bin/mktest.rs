/// mktest — generate deterministic test data under testdata/
///
/// Run with: cargo run --bin mktest
///
/// Wipes and recreates testdata/ from scratch. Expected duplicate groups:
///   "hello world\n"   — 3 files (4 with --hidden)  — 12 bytes each
///   "roses are red\n" — 2 files                    — 15 bytes each
///   128 KB 0xAB block — 2 files                    — 131072 bytes each
///
/// Symlink layout (Unix only):
///   testdata/gamma/alpha_link -> ../alpha
///   Used to test --follow-symlinks behaviour.

use std::fs;
use std::io::Write;
use std::path::Path;

fn main() {
    let root = Path::new("testdata");

    // Wipe and recreate
    if root.exists() {
        fs::remove_dir_all(root).expect("remove testdata");
    }
    fs::create_dir_all(root).expect("create testdata");

    // ── alpha/ ────────────────────────────────────────────────────────────────
    let alpha = root.join("alpha");
    fs::create_dir_all(&alpha).unwrap();
    write_file(&alpha.join("hello.txt"), b"hello world\n");
    write_file(&alpha.join("unique_a.txt"), b"unique content alpha\n");

    let alpha_nested = alpha.join("nested");
    fs::create_dir_all(&alpha_nested).unwrap();
    write_file(&alpha_nested.join("hello_copy.txt"), b"hello world\n");
    write_file(&alpha_nested.join("unique_b.txt"), b"unique content beta\n");

    // ── beta/ ─────────────────────────────────────────────────────────────────
    let beta = root.join("beta");
    fs::create_dir_all(&beta).unwrap();
    write_file(&beta.join("hello_again.txt"), b"hello world\n");
    write_file(&beta.join("unique_c.txt"), b"unique content gamma\n");

    let beta_subdir = beta.join("subdir");
    fs::create_dir_all(&beta_subdir).unwrap();
    write_file(&beta_subdir.join("poem.txt"), b"roses are red\n");
    write_file(&beta_subdir.join("unique_d.txt"), b"unique content delta\n");

    // ── gamma/ ────────────────────────────────────────────────────────────────
    let gamma = root.join("gamma");
    fs::create_dir_all(&gamma).unwrap();
    write_file(&gamma.join("poem_copy.txt"), b"roses are red\n");

    // Symlink: gamma/alpha_link -> ../alpha
    // Points at testdata/alpha/ from inside testdata/gamma/.
    // Used to exercise --follow-symlinks (with canonical-path dedup) and
    // the default behaviour of not following symlinks.
    #[cfg(unix)]
    std::os::unix::fs::symlink("../alpha", &gamma.join("alpha_link"))
        .unwrap_or_else(|e| eprintln!("warning: could not create symlink: {e}"));

    // ── large/ ────────────────────────────────────────────────────────────────
    let large = root.join("large");
    fs::create_dir_all(&large).unwrap();
    let big_data = vec![0xABu8; 128 * 1024];
    write_file(&large.join("big.bin"), &big_data);
    write_file(&large.join("big_copy.bin"), &big_data);

    // ── hidden/ ───────────────────────────────────────────────────────────────
    let hidden = root.join("hidden");
    fs::create_dir_all(&hidden).unwrap();
    write_file(&hidden.join(".hidden_dup.txt"), b"hello world\n");
    write_file(&hidden.join("visible.txt"), b"visible only\n");

    // ── Summary ───────────────────────────────────────────────────────────────
    let db = "testdata/fdedupe.db";
    println!("Test data created under testdata/");
    println!();
    println!("Expected duplicate groups (without --hidden):");
    println!("  \"hello world\\n\"    3 files   12 bytes each");
    println!("  \"roses are red\\n\"  2 files   15 bytes each");
    println!("  128 KB 0xAB block  2 files   131072 bytes each");
    println!();
    println!("Symlink (Unix): testdata/gamma/alpha_link -> ../alpha");
    println!();
    println!("Test commands (DB isolated in testdata/):");
    println!("  cargo run -- --db {db} scan testdata --recursive");
    println!("  cargo run -- --db {db} list testdata");
    println!("  cargo run -- --db {db} list testdata --interactive");
    println!("  cargo run -- --db {db} remove --dry-run");
    println!();
    println!("With --hidden:");
    println!("  cargo run -- --db {db} scan testdata --recursive --hidden");
    println!("  (\"hello world\\n\" becomes 4 files)");
    println!();
    println!("Symlink tests (see doc/test-plan/symlinks.md):");
    println!("  cargo run -- --db {db} scan testdata --recursive               # gamma/alpha_link ignored");
    println!("  cargo run -- --db {db} scan testdata --recursive --follow-symlinks  # link followed, no double-count");
}

fn write_file(path: &Path, content: &[u8]) {
    let mut f = fs::File::create(path)
        .unwrap_or_else(|e| panic!("create {}: {}", path.display(), e));
    f.write_all(content)
        .unwrap_or_else(|e| panic!("write {}: {}", path.display(), e));
}
