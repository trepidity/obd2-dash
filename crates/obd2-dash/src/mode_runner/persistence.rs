use super::capability::CapabilityKey;
use super::store::CapabilityStore;
use anyhow::Result;
use chrono::Utc;
use obd2_db::models::{
    CapabilityContext, CapabilityOutcome, CapabilityRecord, CapabilitySetReplacement, OutcomeUpdate,
};
use std::collections::BTreeMap;

/// Bounded, latest-value persistence. At most one batch is retained between
/// flushes, so slow SQLite cannot create an unbounded write queue.
pub struct Persistence<S: CapabilityStore> {
    store: S,
    vin: String,
    context: CapabilityContext,
    set_id: Option<String>,
    next_seq: i64,
    pending: BTreeMap<CapabilityKey, CapabilityRecord>,
}

impl<S: CapabilityStore> Persistence<S> {
    pub fn new(store: S, vin: impl Into<String>, context: CapabilityContext) -> Self {
        Self {
            store,
            vin: vin.into(),
            context,
            set_id: None,
            next_seq: 1,
            pending: BTreeMap::new(),
        }
    }
    pub async fn replace(&mut self, records: Vec<CapabilityRecord>) -> Result<String> {
        if let Some(max) = records.iter().map(|r| r.observation_seq).max() {
            self.next_seq = max.saturating_add(1);
        }
        let replacement = CapabilitySetReplacement {
            vin: self.vin.clone(),
            context: self.context.clone(),
            completed_at: Utc::now().to_rfc3339(),
            records,
        };
        let id = self.store.replace(&replacement).await?;
        self.set_id = Some(id.clone());
        Ok(id)
    }
    pub fn observe(
        &mut self,
        key: CapabilityKey,
        outcome: CapabilityOutcome,
        error_code: Option<String>,
    ) -> CapabilityRecord {
        let record = CapabilityRecord {
            kind: key.kind,
            request_id: key.request_id.clone(),
            module: key.module.clone(),
            outcome,
            observation_seq: self.next_seq,
            rtt_ms: None,
            attempted_at: Utc::now().to_rfc3339(),
            error_code,
        };
        self.next_seq = self.next_seq.saturating_add(1);
        self.pending.insert(key, record.clone());
        record
    }
    pub async fn flush(&mut self) -> Result<Option<OutcomeUpdate>> {
        let Some(set_id) = self.set_id.as_deref() else {
            self.pending.clear();
            return Ok(None);
        };
        if self.pending.is_empty() {
            return Ok(None);
        }
        let records: Vec<_> = self.pending.values().cloned().collect();
        let result = self
            .store
            .update_outcomes(&self.vin, set_id, &records)
            .await?;
        self.pending.clear();
        Ok(Some(result))
    }
    pub fn set_id(&self) -> Option<&str> {
        self.set_id.as_deref()
    }
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mode_runner::SqliteCapabilityStore;
    use obd2_db::Database;
    #[tokio::test]
    async fn replacement_installs_set_id_before_updates() {
        let store = SqliteCapabilityStore::from_database(Database::open_in_memory().unwrap());
        let ctx = CapabilityContext {
            protocol: "p".into(),
            profile_id: "g".into(),
            probe_schema_version: 1,
            probe_fingerprint: "f".into(),
        };
        let mut p = Persistence::new(store, "VIN", ctx);
        let key = CapabilityKey::new(CapabilityKind::Pid, "010C", "broadcast");
        let id = p.replace(vec![]).await.unwrap();
        assert_eq!(p.set_id(), Some(id.as_str()));
        p.observe(key, CapabilityOutcome::Supported, None);
        assert_eq!(p.flush().await.unwrap(), Some(OutcomeUpdate::Applied));
    }
}
