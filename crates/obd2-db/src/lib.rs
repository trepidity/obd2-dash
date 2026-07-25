pub mod models;
pub mod seed;

use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Result};
use rusqlite::{Connection, OptionalExtension};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use models::{
    CapabilityContext, CapabilityKind, CapabilityLoad, CapabilityOutcome, CapabilityRecord,
    CapabilitySetReplacement, DefaultThreshold, EngineFamily, OutcomeUpdate, PidThreshold,
    ResolvedThreshold, VehicleCapabilitySet, VehicleInfo,
};

static SET_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

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

const CAPABILITY_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS vehicle_capability_sets (
    vin                  TEXT PRIMARY KEY CHECK(length(vin) = 17),
    set_id               TEXT NOT NULL CHECK(length(set_id) > 0),
    protocol             TEXT NOT NULL CHECK(length(protocol) > 0),
    profile_id           TEXT NOT NULL CHECK(length(profile_id) > 0),
    probe_schema_version INTEGER NOT NULL CHECK(probe_schema_version >= 1),
    probe_fingerprint    TEXT NOT NULL CHECK(length(probe_fingerprint) > 0),
    scan_completed_at    TEXT NOT NULL CHECK(length(scan_completed_at) > 0)
);

CREATE TABLE IF NOT EXISTS vehicle_capabilities (
    vin               TEXT NOT NULL CHECK(length(vin) = 17)
                      REFERENCES vehicle_capability_sets(vin)
                      ON DELETE CASCADE,
    kind              TEXT NOT NULL
                      CHECK(kind IN ('pid', 'profile_signal', 'service')),
    request_id        TEXT NOT NULL CHECK(length(request_id) > 0),
    module            TEXT NOT NULL CHECK(length(module) > 0),
    outcome           TEXT NOT NULL
                      CHECK(outcome IN ('supported', 'unsupported', 'unverified')),
    observation_seq   INTEGER NOT NULL CHECK(observation_seq >= 0),
    rtt_ms            INTEGER CHECK(rtt_ms IS NULL OR rtt_ms >= 0),
    last_attempted_at TEXT NOT NULL CHECK(length(last_attempted_at) > 0),
    last_error_code   TEXT,
    PRIMARY KEY (vin, kind, request_id, module)
);
"#;

fn initialize_schema(conn: &mut Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > 1 {
        bail!("database schema version {version} is newer than supported version 1");
    }
    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA)?;
    tx.execute_batch(CAPABILITY_SCHEMA)?;
    tx.execute_batch("PRAGMA user_version = 1")?;
    tx.commit()?;
    Ok(())
}

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) a SQLite database at the given path and run migrations.
    pub fn open(path: &Path) -> Result<Self> {
        tracing::info!(target: "obd2::db", "Opening database at {}", path.display());
        let mut conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        initialize_schema(&mut conn)?;
        tracing::debug!(target: "obd2::db", "Database schema initialized");
        Ok(Self { conn })
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        tracing::debug!(target: "obd2::db", "Opening in-memory database");
        let mut conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        initialize_schema(&mut conn)?;
        Ok(Self { conn })
    }

    /// Insert or update an engine family. Returns the row id.
    /// Uses ON CONFLICT to preserve the id (avoiding FK violations from vehicles).
    pub fn upsert_engine_family(&self, ef: &EngineFamily) -> Result<i64> {
        tracing::debug!(target: "obd2::db", "Upsert engine family: {} ({}L {})", ef.family_code, ef.displacement_l, ef.fuel_type);
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
        tracing::debug!(target: "obd2::db", "Upsert vehicle: VIN={} {}", v.vin, v.display_name());
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

    /// Look up a vehicle by VIN pattern — matches positions 1-8 (WMI + VDS = make/model/body/engine)
    /// and position 10 (model year). Returns the matched vehicle info with the actual VIN substituted.
    pub fn get_vehicle_by_vin_pattern(&self, vin: &str) -> Result<Option<VehicleInfo>> {
        if vin.len() < 11 {
            return Ok(None);
        }

        let mut stmt = self.conn.prepare(
            "SELECT v.vin, v.year, v.make, v.model, v.trim, v.engine_family_id, \
                    ef.family_code, v.transmission_type, v.drive_type, \
                    v.fuel_type, v.displacement_l, v.cylinders \
             FROM vehicles v \
             LEFT JOIN engine_families ef ON v.engine_family_id = ef.id \
             WHERE substr(v.vin, 1, 8) = substr(?1, 1, 8) \
               AND substr(v.vin, 10, 1) = substr(?1, 10, 1) \
             LIMIT 1",
        )?;

        let result = stmt.query_row(rusqlite::params![vin], |row| {
            Ok(VehicleInfo {
                vin: vin.to_string(), // Use the actual VIN, not the seeded one
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
        tracing::debug!(
            target: "obd2::db",
            "Resolving thresholds for {} PIDs (vin={:?}, engine_family={:?})",
            pid_codes.len(),
            vin,
            engine_family_code,
        );
        let mut map = HashMap::new();
        for &code in pid_codes {
            if let Some(t) = self.resolve_threshold(code, vin, engine_family_code)? {
                map.insert(code, t);
            }
        }
        tracing::info!(target: "obd2::db", "Resolved {} thresholds", map.len());
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

        let result = stmt.query_row(rusqlite::params![scope_type, scope_id, pid_code], |row| {
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
        });

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

    pub fn load_capability_set(
        &self,
        vin: &str,
        context: &CapabilityContext,
    ) -> Result<CapabilityLoad> {
        let parent = self
            .conn
            .query_row(
                "SELECT set_id, protocol, profile_id, probe_schema_version,
                        probe_fingerprint, scan_completed_at
                 FROM vehicle_capability_sets WHERE vin = ?1",
                [vin],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        CapabilityContext {
                            protocol: row.get(1)?,
                            profile_id: row.get(2)?,
                            probe_schema_version: row.get(3)?,
                            probe_fingerprint: row.get(4)?,
                        },
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((set_id, stored_context, completed_at)) = parent else {
            return Ok(CapabilityLoad::Miss);
        };
        if stored_context != *context {
            return Ok(CapabilityLoad::ContextMismatch);
        }

        let mut stmt = self.conn.prepare(
            "SELECT kind, request_id, module, outcome, observation_seq, rtt_ms,
                    last_attempted_at, last_error_code
             FROM vehicle_capabilities
             WHERE vin = ?1
             ORDER BY kind, request_id, module",
        )?;
        let records = stmt
            .query_map([vin], |row| {
                let kind: String = row.get(0)?;
                let outcome: String = row.get(3)?;
                Ok(CapabilityRecord {
                    kind: CapabilityKind::parse(&kind).ok_or_else(|| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            format!("invalid capability kind {kind}").into(),
                        )
                    })?,
                    request_id: row.get(1)?,
                    module: row.get(2)?,
                    outcome: CapabilityOutcome::parse(&outcome).ok_or_else(|| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            format!("invalid capability outcome {outcome}").into(),
                        )
                    })?,
                    observation_seq: row.get(4)?,
                    rtt_ms: row.get(5)?,
                    attempted_at: row.get(6)?,
                    error_code: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(CapabilityLoad::Hit(VehicleCapabilitySet {
            vin: vin.to_string(),
            set_id,
            context: stored_context,
            completed_at,
            records,
        }))
    }

    pub fn replace_capability_set(
        &mut self,
        replacement: &CapabilitySetReplacement,
    ) -> Result<String> {
        let set_id = new_set_id();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO vehicle_capability_sets
                (vin, set_id, protocol, profile_id, probe_schema_version,
                 probe_fingerprint, scan_completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(vin) DO UPDATE SET
                set_id = excluded.set_id,
                protocol = excluded.protocol,
                profile_id = excluded.profile_id,
                probe_schema_version = excluded.probe_schema_version,
                probe_fingerprint = excluded.probe_fingerprint,
                scan_completed_at = excluded.scan_completed_at",
            rusqlite::params![
                replacement.vin,
                set_id,
                replacement.context.protocol,
                replacement.context.profile_id,
                replacement.context.probe_schema_version,
                replacement.context.probe_fingerprint,
                replacement.completed_at,
            ],
        )?;
        tx.execute(
            "DELETE FROM vehicle_capabilities WHERE vin = ?1",
            [&replacement.vin],
        )?;
        for record in &replacement.records {
            insert_capability_record(&tx, &replacement.vin, record)?;
        }
        tx.commit()?;
        Ok(set_id)
    }

    pub fn update_capability_outcomes(
        &mut self,
        vin: &str,
        set_id: &str,
        records: &[CapabilityRecord],
    ) -> Result<OutcomeUpdate> {
        let tx = self.conn.transaction()?;
        let current_set_id: Option<String> = tx
            .query_row(
                "SELECT set_id FROM vehicle_capability_sets WHERE vin = ?1",
                [vin],
                |row| row.get(0),
            )
            .optional()?;
        if current_set_id.as_deref() != Some(set_id) {
            tx.rollback()?;
            return Ok(OutcomeUpdate::StaleSet);
        }
        for record in records {
            insert_or_update_capability_record(&tx, vin, record)?;
        }
        tx.commit()?;
        Ok(OutcomeUpdate::Applied)
    }
}

fn new_set_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = SET_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:032x}-{counter:016x}")
}

fn insert_capability_record(
    tx: &rusqlite::Transaction<'_>,
    vin: &str,
    record: &CapabilityRecord,
) -> Result<()> {
    tx.execute(
        "INSERT INTO vehicle_capabilities
            (vin, kind, request_id, module, outcome, observation_seq, rtt_ms,
             last_attempted_at, last_error_code)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            vin,
            record.kind.as_str(),
            record.request_id,
            record.module,
            record.outcome.as_str(),
            record.observation_seq,
            record.rtt_ms,
            record.attempted_at,
            record.error_code,
        ],
    )?;
    Ok(())
}

fn insert_or_update_capability_record(
    tx: &rusqlite::Transaction<'_>,
    vin: &str,
    record: &CapabilityRecord,
) -> Result<()> {
    tx.execute(
        "INSERT INTO vehicle_capabilities
            (vin, kind, request_id, module, outcome, observation_seq, rtt_ms,
             last_attempted_at, last_error_code)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(vin, kind, request_id, module) DO UPDATE SET
            outcome = excluded.outcome,
            observation_seq = excluded.observation_seq,
            rtt_ms = excluded.rtt_ms,
            last_attempted_at = excluded.last_attempted_at,
            last_error_code = excluded.last_error_code
         WHERE excluded.observation_seq >= vehicle_capabilities.observation_seq",
        rusqlite::params![
            vin,
            record.kind.as_str(),
            record.request_id,
            record.module,
            record.outcome.as_str(),
            record.observation_seq,
            record.rtt_ms,
            record.attempted_at,
            record.error_code,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability_context(fingerprint: &str) -> CapabilityContext {
        CapabilityContext {
            protocol: "j1850_vpw".into(),
            profile_id: "lly".into(),
            probe_schema_version: 1,
            probe_fingerprint: fingerprint.into(),
        }
    }

    fn capability_record(seq: i64, outcome: CapabilityOutcome) -> CapabilityRecord {
        CapabilityRecord {
            kind: CapabilityKind::Pid,
            request_id: "010C".into(),
            module: "broadcast".into(),
            outcome,
            observation_seq: seq,
            rtt_ms: Some(120),
            attempted_at: format!("2026-07-24T00:00:{seq:02}Z"),
            error_code: None,
        }
    }

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
        let version: i64 = db
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn capability_set_round_trips_and_context_mismatch_is_visible() {
        let mut db = Database::open_in_memory().unwrap();
        let replacement = CapabilitySetReplacement {
            vin: "1GCHK23224F000001".into(),
            context: capability_context("a"),
            completed_at: "2026-07-24T00:00:00Z".into(),
            records: vec![capability_record(1, CapabilityOutcome::Supported)],
        };
        let set_id = db.replace_capability_set(&replacement).unwrap();
        let loaded = db
            .load_capability_set(&replacement.vin, &replacement.context)
            .unwrap();
        let CapabilityLoad::Hit(set) = loaded else {
            panic!("expected capability cache hit");
        };
        assert_eq!(set.set_id, set_id);
        assert_eq!(set.records, replacement.records);
        assert!(matches!(
            db.load_capability_set(&replacement.vin, &capability_context("b"))
                .unwrap(),
            CapabilityLoad::ContextMismatch
        ));
    }

    #[test]
    fn replacement_is_atomic_and_stale_updates_are_rejected() {
        let mut db = Database::open_in_memory().unwrap();
        let old = CapabilitySetReplacement {
            vin: "1GCHK23224F000001".into(),
            context: capability_context("old"),
            completed_at: "2026-07-24T00:00:00Z".into(),
            records: vec![capability_record(1, CapabilityOutcome::Supported)],
        };
        let old_set_id = db.replace_capability_set(&old).unwrap();

        let duplicate = capability_record(2, CapabilityOutcome::Unsupported);
        let failed = CapabilitySetReplacement {
            context: capability_context("new"),
            records: vec![duplicate.clone(), duplicate],
            ..old.clone()
        };
        assert!(db.replace_capability_set(&failed).is_err());
        assert!(matches!(
            db.load_capability_set(&old.vin, &old.context).unwrap(),
            CapabilityLoad::Hit(_)
        ));
        assert!(matches!(
            db.update_capability_outcomes(
                &old.vin,
                "not-the-current-set",
                &[capability_record(3, CapabilityOutcome::Unsupported)]
            )
            .unwrap(),
            OutcomeUpdate::StaleSet
        ));
        assert!(matches!(
            db.update_capability_outcomes(
                &old.vin,
                &old_set_id,
                &[capability_record(0, CapabilityOutcome::Unsupported)]
            )
            .unwrap(),
            OutcomeUpdate::Applied
        ));
        let CapabilityLoad::Hit(set) = db.load_capability_set(&old.vin, &old.context).unwrap()
        else {
            panic!("expected old capability set");
        };
        assert_eq!(set.records[0].outcome, CapabilityOutcome::Supported);
    }

    #[test]
    fn module_is_non_nullable_and_duplicate_keys_conflict() {
        let db = Database::open_in_memory().unwrap();
        assert!(db
            .conn
            .execute(
                "INSERT INTO vehicle_capability_sets
                 (vin, set_id, protocol, profile_id, probe_schema_version,
                  probe_fingerprint, scan_completed_at)
                 VALUES ('1GCHK23224F000001', 'set', 'can_11bit_500',
                         'generic', 1, 'fp', 'now')",
                [],
            )
            .is_ok());
        let first = db.conn.execute(
            "INSERT INTO vehicle_capabilities
             (vin, kind, request_id, module, outcome, observation_seq,
              last_attempted_at)
             VALUES ('1GCHK23224F000001', 'pid', '010C', 'broadcast',
                     'supported', 1, 'now')",
            [],
        );
        assert!(first.is_ok());
        assert!(db
            .conn
            .execute(
                "INSERT INTO vehicle_capabilities
                 (vin, kind, request_id, module, outcome, observation_seq,
                  last_attempted_at)
                 VALUES ('1GCHK23224F000001', 'pid', '010C', NULL,
                         'supported', 2, 'now')",
                [],
            )
            .is_err());
        assert!(db
            .conn
            .execute(
                "INSERT INTO vehicle_capabilities
                 (vin, kind, request_id, module, outcome, observation_seq,
                  last_attempted_at)
                 VALUES ('1GCHK23224F000001', 'pid', '010C', 'broadcast',
                         'supported', 2, 'now')",
                [],
            )
            .is_err());
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
    fn test_vin_pattern_match() {
        let db = Database::open_in_memory().unwrap();
        seed::seed_all(&db).unwrap();

        // The seeded Malibu VIN is 1G1ZD5ST0LF000001
        // A real VIN sharing positions 1-8 and position 10 should match
        let real_vin = "1G1ZD5ST2LF000000";
        let vehicle = db.get_vehicle_by_vin_pattern(real_vin).unwrap().unwrap();
        assert_eq!(vehicle.vin, real_vin); // Should have the actual VIN
        assert_eq!(vehicle.make.as_deref(), Some("Chevrolet"));
        assert_eq!(vehicle.model.as_deref(), Some("Malibu"));
        assert_eq!(vehicle.year, Some(2020));

        // A completely different VIN should not match
        let unknown_vin = "5YJSA1E26MF000001";
        assert!(db
            .get_vehicle_by_vin_pattern(unknown_vin)
            .unwrap()
            .is_none());

        // Short VIN should return None gracefully
        assert!(db.get_vehicle_by_vin_pattern("ABC").unwrap().is_none());
    }

    #[test]
    fn test_default_threshold_fallback() {
        let db = Database::open_in_memory().unwrap();
        seed::seed_all(&db).unwrap();

        // PID 0x04 (engine load) with no overrides — should return default
        let threshold = db.resolve_threshold(0x04, None, None).unwrap().unwrap();
        assert!((threshold.max_value - 100.0).abs() < 0.01);
    }

    #[test]
    fn rule6_engine_family_override_survives_default_change() {
        let db = Database::open_in_memory().unwrap();
        seed::seed_all(&db).unwrap();

        let default_threshold = db.resolve_threshold(0x0C, None, None).unwrap().unwrap();
        let mini_threshold = db
            .resolve_threshold(0x0C, Some("WMWRE33546T000001"), Some("W11B16"))
            .unwrap()
            .unwrap();

        assert!(
            (default_threshold.high_warning.unwrap() - mini_threshold.high_warning.unwrap()).abs()
                > 1.0,
            "Mini RPM high_warning should differ from default",
        );
        assert!(
            (mini_threshold.high_warning.unwrap() - 6200.0).abs() < 0.01,
            "Mini RPM high_warning should be 6200",
        );
    }

    #[test]
    fn rule6_resolve_all_returns_per_vehicle_thresholds() {
        let db = Database::open_in_memory().unwrap();
        seed::seed_all(&db).unwrap();

        let codes: Vec<u8> = vec![0x04, 0x05, 0x0C, 0x0D, 0x11];

        let duramax = db
            .resolve_all_thresholds(Some("1GCHK23164F000001"), Some("LLY"), &codes)
            .unwrap();
        let mini = db
            .resolve_all_thresholds(Some("WMWRE33546T000001"), Some("W11B16"), &codes)
            .unwrap();

        assert!(!duramax.is_empty());
        assert!(!mini.is_empty());

        if let (Some(d), Some(m)) = (duramax.get(&0x0C), mini.get(&0x0C)) {
            assert_ne!(d.high_warning, m.high_warning);
        }
    }

    #[test]
    fn rule6_unknown_vehicle_falls_back_to_defaults() {
        let db = Database::open_in_memory().unwrap();
        seed::seed_all(&db).unwrap();

        let threshold = db
            .resolve_threshold(0x0C, Some("UNKNOWN_VIN_12345"), None)
            .unwrap()
            .unwrap();

        assert!(threshold.high_warning.is_some());
        assert!(threshold.high_critical.is_some());
    }
}
