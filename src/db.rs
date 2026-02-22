use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;

pub struct Db {
    conn: Connection,
}

// ── Row types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DirectoryRow {
    pub id: i64,
    pub canonical_path: String,
    pub last_scanned: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct FileRow {
    pub id: i64,
    pub directory_id: i64,
    pub name: String,
    pub canonical_path: String,
    pub size: i64,
    pub modified_at: i64,
    pub fast_hash: Option<String>,
    pub full_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RuleRow {
    pub id: i64,
    pub pattern: String,
    pub priority: i64,
}

#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    pub full_hash: String,
    pub files: Vec<FileRow>,
}

// ── Open / schema ────────────────────────────────────────────────────────────

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("opening database {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self { conn };
        db.create_schema()?;
        Ok(db)
    }

    fn create_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS directories (
                id             INTEGER PRIMARY KEY,
                canonical_path TEXT NOT NULL UNIQUE,
                last_scanned   INTEGER
            );

            CREATE TABLE IF NOT EXISTS files (
                id             INTEGER PRIMARY KEY,
                directory_id   INTEGER NOT NULL REFERENCES directories(id) ON DELETE CASCADE,
                name           TEXT NOT NULL,
                canonical_path TEXT NOT NULL UNIQUE,
                size           INTEGER NOT NULL,
                modified_at    INTEGER NOT NULL,
                fast_hash      TEXT,
                full_hash      TEXT,
                UNIQUE(directory_id, name)
            );

            CREATE INDEX IF NOT EXISTS idx_files_size_fast ON files(size, fast_hash);
            CREATE INDEX IF NOT EXISTS idx_files_full_hash ON files(full_hash);
            CREATE INDEX IF NOT EXISTS idx_files_directory  ON files(directory_id);

            CREATE TABLE IF NOT EXISTS rules (
                id       INTEGER PRIMARY KEY,
                pattern  TEXT NOT NULL,
                priority INTEGER NOT NULL DEFAULT 0
            );
            ",
        )?;
        Ok(())
    }

    // ── Directories ──────────────────────────────────────────────────────────

    pub fn get_directory(&self, canonical_path: &str) -> Result<Option<DirectoryRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, canonical_path, last_scanned FROM directories WHERE canonical_path = ?1",
        )?;
        let mut rows = stmt.query(params![canonical_path])?;
        if let Some(row) = rows.next()? {
            Ok(Some(DirectoryRow {
                id: row.get(0)?,
                canonical_path: row.get(1)?,
                last_scanned: row.get(2)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get or insert a directory row; returns its id.
    pub fn upsert_directory(&self, canonical_path: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO directories(canonical_path) VALUES(?1)",
            params![canonical_path],
        )?;
        let id: i64 = self.conn.query_row(
            "SELECT id FROM directories WHERE canonical_path = ?1",
            params![canonical_path],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    pub fn set_directory_scanned(&self, id: i64, timestamp: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE directories SET last_scanned = ?1 WHERE id = ?2",
            params![timestamp, id],
        )?;
        Ok(())
    }

    pub fn child_directories(&self, parent_path: &str) -> Result<Vec<DirectoryRow>> {
        // Direct children only: one extra path component, no trailing slash variant
        let prefix = format!("{}/", parent_path.trim_end_matches('/'));
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, canonical_path, last_scanned FROM directories
             WHERE canonical_path LIKE ?1 ESCAPE '\\'
               AND canonical_path NOT LIKE ?2 ESCAPE '\\'",
        )?;
        // matches prefix + one segment (no further slash)
        let like_direct = format!("{}%", escape_like(&prefix));
        let like_nested = format!("{}%/%", escape_like(&prefix));
        let rows = stmt
            .query_map(params![like_direct, like_nested], |r| {
                Ok(DirectoryRow {
                    id: r.get(0)?,
                    canonical_path: r.get(1)?,
                    last_scanned: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ── Files ────────────────────────────────────────────────────────────────

    pub fn files_in_directory(&self, directory_id: i64) -> Result<Vec<FileRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, directory_id, name, canonical_path, size, modified_at, fast_hash, full_hash
             FROM files WHERE directory_id = ?1",
        )?;
        let rows = stmt
            .query_map(params![directory_id], file_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn upsert_file(
        &self,
        directory_id: i64,
        name: &str,
        canonical_path: &str,
        size: i64,
        modified_at: i64,
        fast_hash: Option<&str>,
        full_hash: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO files(directory_id, name, canonical_path, size, modified_at, fast_hash, full_hash)
             VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(canonical_path) DO UPDATE SET
               directory_id = excluded.directory_id,
               name         = excluded.name,
               size         = excluded.size,
               modified_at  = excluded.modified_at,
               fast_hash    = excluded.fast_hash,
               full_hash    = excluded.full_hash",
            params![directory_id, name, canonical_path, size, modified_at, fast_hash, full_hash],
        )?;
        let id: i64 = self.conn.query_row(
            "SELECT id FROM files WHERE canonical_path = ?1",
            params![canonical_path],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    pub fn update_fast_hash(&self, id: i64, fast_hash: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET fast_hash = ?1, full_hash = NULL WHERE id = ?2",
            params![fast_hash, id],
        )?;
        Ok(())
    }

    pub fn update_full_hash(&self, id: i64, full_hash: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET full_hash = ?1 WHERE id = ?2",
            params![full_hash, id],
        )?;
        Ok(())
    }

    pub fn delete_file(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM files WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn delete_file_by_path(&self, canonical_path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM files WHERE canonical_path = ?1",
            params![canonical_path],
        )?;
        Ok(())
    }

    /// Find files that share the same (size, fast_hash) and are missing a full_hash.
    pub fn candidates_needing_full_hash(&self) -> Result<Vec<FileRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, directory_id, name, canonical_path, size, modified_at, fast_hash, full_hash
             FROM files
             WHERE full_hash IS NULL
               AND fast_hash IS NOT NULL
               AND size > 0
               AND (size, fast_hash) IN (
                   SELECT size, fast_hash FROM files
                   WHERE fast_hash IS NOT NULL
                   GROUP BY size, fast_hash
                   HAVING COUNT(*) > 1
               )",
        )?;
        let rows = stmt
            .query_map([], file_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ── Duplicates ───────────────────────────────────────────────────────────

    pub fn duplicate_groups(&self) -> Result<Vec<DuplicateGroup>> {
        // Get all hashes that appear more than once
        let mut hash_stmt = self.conn.prepare_cached(
            "SELECT full_hash FROM files WHERE full_hash IS NOT NULL
             GROUP BY full_hash HAVING COUNT(*) > 1",
        )?;
        let hashes: Vec<String> = hash_stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut groups = Vec::new();
        for full_hash in hashes {
            let files = self.files_with_full_hash(&full_hash)?;
            groups.push(DuplicateGroup { full_hash, files });
        }
        Ok(groups)
    }

    pub fn files_with_full_hash(&self, full_hash: &str) -> Result<Vec<FileRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, directory_id, name, canonical_path, size, modified_at, fast_hash, full_hash
             FROM files WHERE full_hash = ?1",
        )?;
        let rows = stmt
            .query_map(params![full_hash], file_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Count and total size of duplicate files under (and including) the given path prefix.
    pub fn duplicate_stats_under(&self, path_prefix: &str) -> Result<(i64, i64)> {
        let prefix = format!("{}/", path_prefix.trim_end_matches('/'));
        // Files directly in the directory or under it
        let like = format!("{}%", escape_like(&prefix));
        let (count, size): (i64, i64) = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(size), 0) FROM files
             WHERE full_hash IS NOT NULL
               AND (canonical_path LIKE ?1 ESCAPE '\\' OR canonical_path LIKE ?2 ESCAPE '\\')
               AND full_hash IN (
                   SELECT full_hash FROM files WHERE full_hash IS NOT NULL
                   GROUP BY full_hash HAVING COUNT(*) > 1
               )",
            params![
                format!("{}%", escape_like(path_prefix)),
                like
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((count, size))
    }

    /// Duplicate files directly in this directory (not subdirs).
    pub fn duplicate_files_in_dir(&self, directory_id: i64) -> Result<Vec<FileRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, directory_id, name, canonical_path, size, modified_at, fast_hash, full_hash
             FROM files
             WHERE directory_id = ?1
               AND full_hash IS NOT NULL
               AND full_hash IN (
                   SELECT full_hash FROM files WHERE full_hash IS NOT NULL
                   GROUP BY full_hash HAVING COUNT(*) > 1
               )",
        )?;
        let rows = stmt
            .query_map(params![directory_id], file_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ── Rules ────────────────────────────────────────────────────────────────

    pub fn all_rules(&self) -> Result<Vec<RuleRow>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, pattern, priority FROM rules ORDER BY priority DESC")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(RuleRow {
                    id: r.get(0)?,
                    pattern: r.get(1)?,
                    priority: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn insert_rule(&self, pattern: &str, priority: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO rules(pattern, priority) VALUES(?1, ?2)",
            params![pattern, priority],
        )?;
        Ok(())
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn file_from_row(r: &rusqlite::Row) -> rusqlite::Result<FileRow> {
    Ok(FileRow {
        id: r.get(0)?,
        directory_id: r.get(1)?,
        name: r.get(2)?,
        canonical_path: r.get(3)?,
        size: r.get(4)?,
        modified_at: r.get(5)?,
        fast_hash: r.get(6)?,
        full_hash: r.get(7)?,
    })
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}
