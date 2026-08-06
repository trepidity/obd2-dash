use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use obd2_db::models::{
    CapabilityContext, CapabilityLoad, CapabilityRecord, CapabilitySetReplacement, OutcomeUpdate,
};
use obd2_db::Database;

#[async_trait]
pub trait CapabilityStore: Send + Sync {
    async fn load(&self, vin: &str, context: &CapabilityContext) -> Result<CapabilityLoad>;

    async fn replace(&self, replacement: &CapabilitySetReplacement) -> Result<String>;

    async fn update_outcomes(
        &self,
        vin: &str,
        set_id: &str,
        records: &[CapabilityRecord],
    ) -> Result<OutcomeUpdate>;

    async fn load_exact_vehicle_fuel_type(&self, vin: &str) -> Result<Option<String>>;
}

#[derive(Clone)]
pub struct SqliteCapabilityStore {
    database: Arc<Mutex<Database>>,
}

impl SqliteCapabilityStore {
    /// Open and migrate SQLite off the Tokio worker thread.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let database = tokio::task::spawn_blocking(move || Database::open(&path))
            .await
            .map_err(|error| anyhow!("database open task failed: {error}"))??;
        Ok(Self::from_database(database))
    }

    pub fn from_database(database: Database) -> Self {
        Self {
            database: Arc::new(Mutex::new(database)),
        }
    }

    async fn with_database<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Database) -> Result<T> + Send + 'static,
    {
        let database = Arc::clone(&self.database);
        tokio::task::spawn_blocking(move || {
            let mut database = database
                .lock()
                .map_err(|_| anyhow!("capability database mutex poisoned"))?;
            operation(&mut database)
        })
        .await
        .map_err(|error| anyhow!("database operation task failed: {error}"))?
    }
}

#[async_trait]
impl CapabilityStore for SqliteCapabilityStore {
    async fn load(&self, vin: &str, context: &CapabilityContext) -> Result<CapabilityLoad> {
        let vin = vin.to_string();
        let context = context.clone();
        self.with_database(move |database| database.load_capability_set(&vin, &context))
            .await
    }

    async fn replace(&self, replacement: &CapabilitySetReplacement) -> Result<String> {
        let replacement = replacement.clone();
        self.with_database(move |database| database.replace_capability_set(&replacement))
            .await
    }

    async fn update_outcomes(
        &self,
        vin: &str,
        set_id: &str,
        records: &[CapabilityRecord],
    ) -> Result<OutcomeUpdate> {
        let vin = vin.to_string();
        let set_id = set_id.to_string();
        let records = records.to_vec();
        self.with_database(move |database| {
            database.update_capability_outcomes(&vin, &set_id, &records)
        })
        .await
    }

    async fn load_exact_vehicle_fuel_type(&self, vin: &str) -> Result<Option<String>> {
        let vin = vin.to_string();
        self.with_database(move |database| {
            Ok(database
                .get_vehicle(&vin)?
                .and_then(|vehicle| vehicle.fuel_type))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use obd2_db::models::{CapabilityKind, CapabilityOutcome};

    fn context() -> CapabilityContext {
        CapabilityContext {
            protocol: "can_11bit_500".into(),
            profile_id: "generic".into(),
            probe_schema_version: 1,
            probe_fingerprint: "test".into(),
        }
    }

    #[tokio::test]
    async fn sqlite_store_round_trips_through_blocking_boundary() {
        let store = SqliteCapabilityStore::from_database(Database::open_in_memory().unwrap());
        let replacement = CapabilitySetReplacement {
            vin: "1GCHK23224F000001".into(),
            context: context(),
            completed_at: "now".into(),
            records: vec![CapabilityRecord {
                kind: CapabilityKind::Pid,
                request_id: "010C".into(),
                module: "broadcast".into(),
                outcome: CapabilityOutcome::Supported,
                observation_seq: 1,
                rtt_ms: None,
                attempted_at: "now".into(),
                error_code: None,
            }],
        };
        let set_id = store.replace(&replacement).await.unwrap();
        let loaded = store
            .load(&replacement.vin, &replacement.context)
            .await
            .unwrap();
        assert!(matches!(loaded, CapabilityLoad::Hit(set) if set.set_id == set_id));
    }

    #[tokio::test]
    async fn exact_fuel_lookup_does_not_use_pattern_matching() {
        let store = SqliteCapabilityStore::from_database(Database::open_in_memory().unwrap());
        assert_eq!(
            store
                .load_exact_vehicle_fuel_type("1GCHK23224F000001")
                .await
                .unwrap(),
            None
        );
    }
}
