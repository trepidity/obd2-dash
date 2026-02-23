pub mod models;
pub mod seed;

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

use models::{DefaultThreshold, EngineFamily, PidThreshold, ResolvedThreshold, VehicleInfo};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS engine_families (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    manufacturer TEXT NOT NULL,
    family_code TEXT NOT NULL UNIQUE,
    displacement_l REAL NOT NULL,
    cylinders INTEGER NOT NULL,
    layout TEXT NOT NULL,
    aspiration TEXT NOT NULL,
    fuel_type TEXT NOT NULL,
    compression_ratio REAL,
    redline_rpm INTEGER,
    idle_rpm_cold INTEGER,
    idle_rpm_warm INTEGER,
    max_power_kw REAL,
    max_torque_nm REAL
);

CREATE TABLE IF NOT EXISTS vehicles (
    vin TEXT PRIMARY KEY,
    year INTEGER,
    make TEXT,
    model TEXT,
    trim TEXT,
    engine_family_id INTEGER REFERENCES engine_families(id),
    transmission_type TEXT,
    drive_type TEXT,
    body_class TEXT,
    fuel_type TEXT,
    displacement_l REAL,
    cylinders INTEGER,
    gvwr_kg REAL,
    supported_pids TEXT,
    last_seen TEXT
);

CREATE TABLE IF NOT EXISTS default_thresholds (
    pid_code INTEGER PRIMARY KEY,
    min_value REAL NOT NULL,
    max_value REAL NOT NULL,
    low_warning REAL,
    high_warning REAL,
    low_critical REAL,
    high_critical REAL,
    unit TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS pid_thresholds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scope_type TEXT NOT NULL CHECK(scope_type IN ('engine_family','make_model','vin')),
    scope_id TEXT NOT NULL,
    pid_code INTEGER NOT NULL,
    min_value REAL,
    max_value REAL,
    low_warning REAL,
    high_warning REAL,
    low_critical REAL,
    high_critical REAL,
    notes TEXT,
    UNIQUE(scope_type, scope_id, pid_code)
);

CREATE TABLE IF NOT EXISTS learned_baselines (
    vin TEXT NOT NULL,
    pid_code INTEGER NOT NULL,
    mean_value REAL NOT NULL,
    std_dev REAL NOT NULL,
    sample_count INTEGER NOT NULL,
    conditions TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (vin, pid_code)
);
"#;

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) a SQLite database at the given path and run migrations.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Insert or update an engine family. Returns the row id.
    /// Uses ON CONFLICT to preserve the id (avoiding FK violations from vehicles).
    pub fn upsert_engine_family(&self, ef: &EngineFamily) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO engine_families \
             (manufacturer, family_code, displacement_l, cylinders, layout, aspiration, \
              fuel_type, compression_ratio, redline_rpm, idle_rpm_cold, idle_rpm_warm, \
              max_power_kw, max_torque_nm) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13) \
             ON CONFLICT(family_code) DO UPDATE SET \
              manufacturer=excluded.manufacturer, \
              displacement_l=excluded.displacement_l, \
              cylinders=excluded.cylinders, \
              layout=excluded.layout, \
              aspiration=excluded.aspiration, \
              fuel_type=excluded.fuel_type, \
              compression_ratio=excluded.compression_ratio, \
              redline_rpm=excluded.redline_rpm, \
              idle_rpm_cold=excluded.idle_rpm_cold, \
              idle_rpm_warm=excluded.idle_rpm_warm, \
              max_power_kw=excluded.max_power_kw, \
              max_torque_nm=excluded.max_torque_nm",
            rusqlite::params![
                ef.manufacturer,
                ef.family_code,
                ef.displacement_l,
                ef.cylinders,
                ef.layout,
                ef.aspiration,
                ef.fuel_type,
                ef.compression_ratio,
                ef.redline_rpm,
                ef.idle_rpm_cold,
                ef.idle_rpm_warm,
                ef.max_power_kw,
                ef.max_torque_nm,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert or replace a vehicle record.
    pub fn upsert_vehicle(&self, v: &VehicleInfo) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO vehicles \
             (vin, year, make, model, trim, engine_family_id, \
              transmission_type, drive_type, fuel_type, displacement_l, cylinders) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![
                v.vin,
                v.year,
                v.make,
                v.model,
                v.trim,
                v.engine_family_id,
                v.transmission_type,
                v.drive_type,
                v.fuel_type,
                v.displacement_l,
                v.cylinders,
            ],
        )?;
        Ok(())
    }

    /// Insert or replace a default threshold.
    pub fn upsert_default_threshold(&self, t: &DefaultThreshold) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO default_thresholds \
             (pid_code, min_value, max_value, low_warning, high_warning, \
              low_critical, high_critical, unit) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![
                t.pid_code,
                t.min_value,
                t.max_value,
                t.low_warning,
                t.high_warning,
                t.low_critical,
                t.high_critical,
                t.unit,
            ],
        )?;
        Ok(())
    }

    /// Insert or replace a scoped PID threshold override.
    pub fn upsert_pid_threshold(&self, t: &PidThreshold) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO pid_thresholds \
             (scope_type, scope_id, pid_code, min_value, max_value, \
              low_warning, high_warning, low_critical, high_critical, notes) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![
                t.scope_type,
                t.scope_id,
                t.pid_code,
                t.min_value,
                t.max_value,
                t.low_warning,
                t.high_warning,
                t.low_critical,
                t.high_critical,
                t.notes,
            ],
        )?;
        Ok(())
    }

    /// Look up a vehicle by VIN.
    pub fn get_vehicle(&self, vin: &str) -> Result<Option<VehicleInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT v.vin, v.year, v.make, v.model, v.trim, v.engine_family_id, \
                    ef.family_code, v.transmission_type, v.drive_type, \
                    v.fuel_type, v.displacement_l, v.cylinders \
             FROM vehicles v \
             LEFT JOIN engine_families ef ON v.engine_family_id = ef.id \
             WHERE v.vin = ?1",
        )?;

        let result = stmt.query_row(rusqlite::params![vin], |row| {
            Ok(VehicleInfo {
                vin: row.get(0)?,
                year: row.get(1)?,
                make: row.get(2)?,
                model: row.get(3)?,
                trim: row.get(4)?,
                engine_family_id: row.get(5)?,
                engine_family_code: row.get(6)?,
                transmission_type: row.get(7)?,
                drive_type: row.get(8)?,
                fuel_type: row.get(9)?,
                displacement_l: row.get(10)?,
                cylinders: row.get(11)?,
            })
        });

        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get engine family code for a vehicle.
    pub fn get_engine_family_code(&self, engine_family_id: i64) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT family_code FROM engine_families WHERE id = ?1")?;
        let result = stmt.query_row(rusqlite::params![engine_family_id], |row| {
            row.get::<_, String>(0)
        });
        match result {
            Ok(code) => Ok(Some(code)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Resolve the effective threshold for a PID given a vehicle context.
    ///
    /// Priority: VIN-specific → engine_family → default
    pub fn resolve_threshold(
        &self,
        pid_code: u8,
        vin: Option<&str>,
        engine_family_code: Option<&str>,
    ) -> Result<Option<ResolvedThreshold>> {
        // Start with the default threshold
        let default = self.get_default_threshold(pid_code)?;
        let base = match default {
            Some(d) => ResolvedThreshold {
                pid_code: d.pid_code,
                min_value: d.min_value,
                max_value: d.max_value,
                low_warning: d.low_warning,
                high_warning: d.high_warning,
                low_critical: d.low_critical,
                high_critical: d.high_critical,
                unit: d.unit,
            },
            None => return Ok(None),
        };

        // Apply engine family overrides
        let after_ef = if let Some(ef_code) = engine_family_code {
            self.apply_override(base, "engine_family", ef_code, pid_code)?
        } else {
            base
        };

        // Apply VIN-specific overrides
        let final_threshold = if let Some(vin) = vin {
            self.apply_override(after_ef, "vin", vin, pid_code)?
        } else {
            after_ef
        };

        Ok(Some(final_threshold))
    }

    /// Resolve thresholds for all known PID codes and return as a map.
    pub fn resolve_all_thresholds(
        &self,
        vin: Option<&str>,
        engine_family_code: Option<&str>,
        pid_codes: &[u8],
    ) -> Result<HashMap<u8, ResolvedThreshold>> {
        let mut map = HashMap::new();
        for &code in pid_codes {
            if let Some(t) = self.resolve_threshold(code, vin, engine_family_code)? {
                map.insert(code, t);
            }
        }
        Ok(map)
    }

    fn get_default_threshold(&self, pid_code: u8) -> Result<Option<DefaultThreshold>> {
        let mut stmt = self.conn.prepare(
            "SELECT pid_code, min_value, max_value, low_warning, high_warning, \
                    low_critical, high_critical, unit \
             FROM default_thresholds WHERE pid_code = ?1",
        )?;

        let result = stmt.query_row(rusqlite::params![pid_code], |row| {
            Ok(DefaultThreshold {
                pid_code: row.get::<_, i32>(0)? as u8,
                min_value: row.get(1)?,
                max_value: row.get(2)?,
                low_warning: row.get(3)?,
                high_warning: row.get(4)?,
                low_critical: row.get(5)?,
                high_critical: row.get(6)?,
                unit: row.get(7)?,
            })
        });

        match result {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn get_pid_threshold(
        &self,
        scope_type: &str,
        scope_id: &str,
        pid_code: u8,
    ) -> Result<Option<PidThreshold>> {
        let mut stmt = self.conn.prepare(
            "SELECT scope_type, scope_id, pid_code, min_value, max_value, \
                    low_warning, high_warning, low_critical, high_critical, notes \
             FROM pid_thresholds \
             WHERE scope_type = ?1 AND scope_id = ?2 AND pid_code = ?3",
        )?;

        let result = stmt.query_row(
            rusqlite::params![scope_type, scope_id, pid_code],
            |row| {
                Ok(PidThreshold {
                    scope_type: row.get(0)?,
                    scope_id: row.get(1)?,
                    pid_code: row.get::<_, i32>(2)? as u8,
                    min_value: row.get(3)?,
                    max_value: row.get(4)?,
                    low_warning: row.get(5)?,
                    high_warning: row.get(6)?,
                    low_critical: row.get(7)?,
                    high_critical: row.get(8)?,
                    notes: row.get(9)?,
                })
            },
        );

        match result {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Apply a scoped override on top of a base ResolvedThreshold.
    /// Only non-NULL fields in the override replace the base.
    fn apply_override(
        &self,
        mut base: ResolvedThreshold,
        scope_type: &str,
        scope_id: &str,
        pid_code: u8,
    ) -> Result<ResolvedThreshold> {
        if let Some(ovr) = self.get_pid_threshold(scope_type, scope_id, pid_code)? {
            if let Some(v) = ovr.min_value {
                base.min_value = v;
            }
            if let Some(v) = ovr.max_value {
                base.max_value = v;
            }
            if ovr.low_warning.is_some() {
                base.low_warning = ovr.low_warning;
            }
            if ovr.high_warning.is_some() {
                base.high_warning = ovr.high_warning;
            }
            if ovr.low_critical.is_some() {
                base.low_critical = ovr.low_critical;
            }
            if ovr.high_critical.is_some() {
                base.high_critical = ovr.high_critical;
            }
        }
        Ok(base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_in_memory() {
        let db = Database::open_in_memory().unwrap();
        // Tables should exist
        let count: i32 = db
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count >= 5);
    }

    #[test]
    fn test_seed_and_resolve() {
        let db = Database::open_in_memory().unwrap();
        seed::seed_all(&db).unwrap();

        // Check that the Mini exists
        let vehicle = db.get_vehicle("WMWRE33546T000001").unwrap().unwrap();
        assert_eq!(vehicle.make.as_deref(), Some("MINI"));
        assert_eq!(vehicle.year, Some(2006));

        // Check threshold resolution — RPM (0x0C) for Mini should use W11B16 override
        let threshold = db
            .resolve_threshold(
                0x0C,
                Some("WMWRE33546T000001"),
                vehicle.engine_family_code.as_deref(),
            )
            .unwrap()
            .unwrap();
        // W11B16 has high_warning = 6200, high_critical = 6800 (from seed)
        assert!((threshold.high_warning.unwrap() - 6200.0).abs() < 0.01);
        assert!((threshold.high_critical.unwrap() - 6800.0).abs() < 0.01);
    }

    #[test]
    fn test_default_threshold_fallback() {
        let db = Database::open_in_memory().unwrap();
        seed::seed_all(&db).unwrap();

        // PID 0x04 (engine load) with no overrides — should return default
        let threshold = db
            .resolve_threshold(0x04, None, None)
            .unwrap()
            .unwrap();
        assert!((threshold.max_value - 100.0).abs() < 0.01);
    }
}
