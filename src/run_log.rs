//! Persistent log of completed zone runs, backed by SQLite via `rusqlite`.
//!
//! A "run" is a single trip: we zone out of one map, run through a second map,
//! and port into a third. The log records the three map ids and how long the
//! run took. Map ids are the game's raw `u32` identifiers; a future table will
//! map those to English names.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, Result, params};

/// A single completed zone run, as stored in the log.
#[derive(Debug, Clone)]
pub struct RunEntry {
    /// Row id in the database.
    pub id: i64,
    /// Map we zoned in from (where the run started).
    pub from_map_id: u32,
    /// Map the run itself took place in.
    pub run_map_id: u32,
    /// Map we ported to at the end of the run.
    pub to_map_id: u32,
    /// How long the run took.
    pub duration: Duration,
    /// Wall-clock time the run was completed.
    pub completed_at: SystemTime,
}

/// Aggregated statistics and personal records for a specific 3-map route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteRecord {
    /// Map we zoned in from (where the run started).
    pub from_map_id: u32,
    /// Map the run itself took place in.
    pub run_map_id: u32,
    /// Map we ported to at the end of the run.
    pub to_map_id: u32,
    /// Total number of times this route has been completed.
    pub run_count: u64,
    /// Fastest recorded time for this route (Personal Best).
    pub pb: Duration,
    /// Average time across all recorded runs of this route.
    pub avg: Duration,
}

/// The run log.
///
/// The SQLite connection lives behind a [`Mutex`] so the whole type is
/// `Send + Sync` (rusqlite's `Connection` is `Send` but not `Sync`). That lets
/// it be stored directly inside the imgui render loop, which hudhook requires
/// to be both.
pub struct RunLog {
    conn: Mutex<Connection>,
}

impl RunLog {
    /// Open (creating if needed) a run log at `path`, creating any missing
    /// parent directories.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            // Best effort: if this fails, `Connection::open` will surface the
            // real error below.
            let _ = std::fs::create_dir_all(parent);
        }
        Self::from_connection(Connection::open(path)?)
    }

    /// Open an in-memory run log. Nothing is persisted; handy for tests.
    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS runs (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                from_map_id  INTEGER NOT NULL,
                run_map_id   INTEGER NOT NULL,
                to_map_id    INTEGER NOT NULL,
                duration_ms  INTEGER NOT NULL,
                completed_at INTEGER NOT NULL
            )",
            [],
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Record a completed run, stamping the completion time as "now". Returns
    /// the stored entry (including its new row id).
    pub fn log_run(
        &self,
        from_map_id: u32,
        run_map_id: u32,
        to_map_id: u32,
        duration: Duration,
    ) -> Result<RunEntry> {
        let completed_at = SystemTime::now();
        let duration_ms = duration.as_millis() as i64;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO runs (from_map_id, run_map_id, to_map_id, duration_ms, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                from_map_id,
                run_map_id,
                to_map_id,
                duration_ms,
                unix_seconds(completed_at)
            ],
        )?;
        Ok(RunEntry {
            id: conn.last_insert_rowid(),
            from_map_id,
            run_map_id,
            to_map_id,
            duration,
            completed_at,
        })
    }

    /// All logged runs, newest first.
    pub fn all(&self) -> Result<Vec<RunEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, from_map_id, run_map_id, to_map_id, duration_ms, completed_at
             FROM runs ORDER BY completed_at DESC, id DESC",
        )?;
        stmt.query_map([], row_to_entry)?.collect()
    }

    /// The `limit` most recent runs, newest first.
    pub fn recent(&self, limit: usize) -> Result<Vec<RunEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, from_map_id, run_map_id, to_map_id, duration_ms, completed_at
             FROM runs ORDER BY completed_at DESC, id DESC LIMIT ?1",
        )?;
        stmt.query_map(params![limit as i64], row_to_entry)?.collect()
    }

    /// Aggregated route records, sorted by most frequently run first.
    pub fn records(&self) -> Result<Vec<RouteRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT from_map_id, run_map_id, to_map_id,
                    COUNT(*) AS run_count,
                    MIN(duration_ms) AS pb_ms,
                    CAST(AVG(duration_ms) AS INTEGER) AS avg_ms
             FROM runs
             GROUP BY from_map_id, run_map_id, to_map_id
             ORDER BY run_count DESC, pb_ms ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(RouteRecord {
                from_map_id: row.get("from_map_id")?,
                run_map_id: row.get("run_map_id")?,
                to_map_id: row.get("to_map_id")?,
                run_count: row.get::<_, i64>("run_count")? as u64,
                pb: Duration::from_millis(row.get::<_, i64>("pb_ms")? as u64),
                avg: Duration::from_millis(row.get::<_, i64>("avg_ms")? as u64),
            })
        })?;
        rows.collect()
    }

    /// All runs for a specific route, fastest first.
    pub fn runs_for_route(&self, from_map_id: u32, run_map_id: u32, to_map_id: u32) -> Result<Vec<RunEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, from_map_id, run_map_id, to_map_id, duration_ms, completed_at
             FROM runs
             WHERE from_map_id = ?1 AND run_map_id = ?2 AND to_map_id = ?3
             ORDER BY duration_ms ASC, completed_at DESC",
        )?;
        stmt.query_map(params![from_map_id, run_map_id, to_map_id], row_to_entry)?
            .collect()
    }

    /// Delete a single run by its row ID.
    pub fn delete_run(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM runs WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Delete all recorded runs for a specific route.
    pub fn delete_route(&self, from_map_id: u32, run_map_id: u32, to_map_id: u32) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM runs WHERE from_map_id = ?1 AND run_map_id = ?2 AND to_map_id = ?3",
            params![from_map_id, run_map_id, to_map_id],
        )?;
        Ok(())
    }

    /// Default on-disk location: `%LOCALAPPDATA%\kaos_zone_timer\zone_runs.db`,
    /// falling back to the current directory if `LOCALAPPDATA` is unset.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("kaos_zone_timer").join("zone_runs.db")
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> Result<RunEntry> {
    Ok(RunEntry {
        id: row.get("id")?,
        from_map_id: row.get("from_map_id")?,
        run_map_id: row.get("run_map_id")?,
        to_map_id: row.get("to_map_id")?,
        duration: Duration::from_millis(row.get::<_, i64>("duration_ms")? as u64),
        completed_at: UNIX_EPOCH + Duration::from_secs(row.get::<_, i64>("completed_at")? as u64),
    })
}

fn unix_seconds(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_and_reads_back_a_run() {
        let log = RunLog::in_memory().unwrap();
        let entry = log.log_run(100, 200, 300, Duration::from_millis(83_400)).unwrap();
        assert_eq!(entry.id, 1);

        let runs = log.all().unwrap();
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.from_map_id, 100);
        assert_eq!(run.run_map_id, 200);
        assert_eq!(run.to_map_id, 300);
        assert_eq!(run.duration, Duration::from_millis(83_400));
    }

    #[test]
    fn recent_returns_newest_first_and_respects_limit() {
        let log = RunLog::in_memory().unwrap();
        for i in 1..=5u32 {
            log.log_run(i, i + 10, i + 20, Duration::from_secs(i as u64)).unwrap();
        }

        let recent = log.recent(3).unwrap();
        assert_eq!(recent.len(), 3);
        // Newest (highest id) first.
        assert_eq!(recent[0].from_map_id, 5);
        assert_eq!(recent[1].from_map_id, 4);
        assert_eq!(recent[2].from_map_id, 3);
    }

    #[test]
    fn records_aggregates_counts_pb_and_avg_sorted_by_count() {
        let log = RunLog::in_memory().unwrap();
        // Route A: 3 runs: 100s, 80s, 120s -> count 3, PB 80s, Avg 100s
        log.log_run(1, 2, 3, Duration::from_secs(100)).unwrap();
        log.log_run(1, 2, 3, Duration::from_secs(80)).unwrap();
        log.log_run(1, 2, 3, Duration::from_secs(120)).unwrap();

        // Route B: 1 run: 50s -> count 1, PB 50s, Avg 50s
        log.log_run(10, 20, 30, Duration::from_secs(50)).unwrap();

        let recs = log.records().unwrap();
        assert_eq!(recs.len(), 2);

        // Most frequent first
        assert_eq!(recs[0].from_map_id, 1);
        assert_eq!(recs[0].run_map_id, 2);
        assert_eq!(recs[0].to_map_id, 3);
        assert_eq!(recs[0].run_count, 3);
        assert_eq!(recs[0].pb, Duration::from_secs(80));
        assert_eq!(recs[0].avg, Duration::from_secs(100));

        assert_eq!(recs[1].from_map_id, 10);
        assert_eq!(recs[1].run_map_id, 20);
        assert_eq!(recs[1].to_map_id, 30);
        assert_eq!(recs[1].run_count, 1);
        assert_eq!(recs[1].pb, Duration::from_secs(50));
        assert_eq!(recs[1].avg, Duration::from_secs(50));
    }

    #[test]
    fn runs_for_route_and_deletions_work() {
        let log = RunLog::in_memory().unwrap();
        let e1 = log.log_run(1, 2, 3, Duration::from_secs(100)).unwrap();
        let e2 = log.log_run(1, 2, 3, Duration::from_secs(80)).unwrap();
        let _e3 = log.log_run(4, 5, 6, Duration::from_secs(50)).unwrap();

        let route_runs = log.runs_for_route(1, 2, 3).unwrap();
        assert_eq!(route_runs.len(), 2);
        assert_eq!(route_runs[0].id, e2.id); // Fastest (80s) first
        assert_eq!(route_runs[1].id, e1.id);

        // Delete single run
        log.delete_run(e2.id).unwrap();
        let remaining = log.runs_for_route(1, 2, 3).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, e1.id);

        // Delete entire route
        log.delete_route(1, 2, 3).unwrap();
        let empty = log.runs_for_route(1, 2, 3).unwrap();
        assert!(empty.is_empty());

        // Other route unaffected
        let other = log.runs_for_route(4, 5, 6).unwrap();
        assert_eq!(other.len(), 1);
    }
}
