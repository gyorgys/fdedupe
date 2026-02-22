use anyhow::Result;
use std::io::Read;
use std::path::Path;

const FAST_HASH_BYTES: usize = 64 * 1024; // 64 KB

/// Hash the first 64 KB of a file (fast, for initial dedup candidate detection).
pub fn fast_hash(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; FAST_HASH_BYTES];
    let n = file.read(&mut buf)?;
    buf.truncate(n);
    Ok(blake3::hash(&buf).to_hex().to_string())
}

/// Hash the entire file content (full, definitive duplicate check).
pub fn full_hash(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}
