//! Stores payments in an append-only log file. Every save appends the
//! payment's latest state as one JSON line, and reads seek straight to the
//! newest line for that payment.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use crate::payment::{IdempotencyKey, Payment, PaymentId, TenantId};
use crate::ports::{PaymentStore, StoreError};

#[derive(Debug)]
pub struct FileStore {
    log: Mutex<Log>,
}

#[derive(Debug)]
struct Log {
    file: File,
    /// Where the newest line for each payment starts.
    offsets: HashMap<(TenantId, PaymentId), u64>,
    by_idempotency_key: HashMap<(TenantId, IdempotencyKey), PaymentId>,
}

impl FileStore {
    /// Opens the log at `path`, creating it if needed, and indexes it.
    ///
    /// A final line without a newline is a write torn by a crash; it is
    /// truncated so the next save starts on a clean line.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(path)?;
        let mut log = Log {
            file,
            offsets: HashMap::new(),
            by_idempotency_key: HashMap::new(),
        };
        let mut reader = BufReader::new(log.file.try_clone()?);
        let mut offset = 0;
        let mut line = String::new();
        loop {
            line.clear();
            let read = reader.read_line(&mut line)?;
            if read == 0 || !line.ends_with('\n') {
                break;
            }
            let payment: Payment = serde_json::from_str(&line)
                .map_err(|source| StoreError::Corrupt { offset, source })?;
            log.index(offset, &payment);
            offset += read as u64;
        }
        log.file.set_len(offset)?;
        Ok(Self {
            log: Mutex::new(log),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Log> {
        self.log
            .lock()
            .expect("a panic while writing leaves the payment log in an unknown state")
    }
}

impl PaymentStore for FileStore {
    fn save(&self, payment: &Payment) -> Result<(), StoreError> {
        let mut line = serde_json::to_string(payment).expect("a payment always encodes as JSON");
        line.push('\n');
        let mut log = self.lock();
        let key = (payment.tenant().clone(), payment.idempotency_key().clone());
        if log
            .by_idempotency_key
            .get(&key)
            .is_some_and(|id| id != payment.id())
        {
            return Err(StoreError::DuplicateKey);
        }
        let offset = log.file.seek(SeekFrom::End(0))?;
        log.file.write_all(line.as_bytes())?;
        log.file.sync_data()?;
        log.index(offset, payment);
        Ok(())
    }

    fn get(&self, tenant: &TenantId, id: &PaymentId) -> Result<Option<Payment>, StoreError> {
        let mut log = self.lock();
        match log.offsets.get(&(tenant.clone(), id.clone())).copied() {
            Some(offset) => log.read_at(offset).map(Some),
            None => Ok(None),
        }
    }

    fn find_by_idempotency_key(
        &self,
        tenant: &TenantId,
        key: &IdempotencyKey,
    ) -> Result<Option<Payment>, StoreError> {
        let id = self
            .lock()
            .by_idempotency_key
            .get(&(tenant.clone(), key.clone()))
            .cloned();
        match id {
            Some(id) => self.get(tenant, &id),
            None => Ok(None),
        }
    }
}

impl Log {
    fn index(&mut self, offset: u64, payment: &Payment) {
        let tenant = payment.tenant().clone();
        self.offsets
            .insert((tenant.clone(), payment.id().clone()), offset);
        self.by_idempotency_key.insert(
            (tenant, payment.idempotency_key().clone()),
            payment.id().clone(),
        );
    }

    fn read_at(&mut self, offset: u64) -> Result<Payment, StoreError> {
        self.file.seek(SeekFrom::Start(offset))?;
        let mut line = String::new();
        BufReader::new(&self.file).read_line(&mut line)?;
        serde_json::from_str(&line).map_err(|source| StoreError::Corrupt { offset, source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::payment::{ProviderOutcome, TenantId};

    fn log_path(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("payments-{}-{name}.log", std::process::id()));
        std::fs::remove_file(&path).ok();
        path
    }

    #[test]
    fn the_latest_state_survives_a_restart() {
        let path = log_path("restart");
        let mut payment = Payment::sample("acme", "order-1");
        let store = FileStore::open(&path).unwrap();
        store.save(&payment).unwrap();
        payment
            .record(ProviderOutcome::Succeeded {
                provider_ref: "ch_1".into(),
            })
            .unwrap();
        store.save(&payment).unwrap();
        drop(store);

        let reopened = FileStore::open(&path).unwrap();

        assert_eq!(
            reopened.get(payment.tenant(), payment.id()).unwrap(),
            Some(payment)
        );
    }

    #[test]
    fn idempotency_keys_are_scoped_to_a_tenant() {
        let store = FileStore::open(&log_path("scoped")).unwrap();
        let acme = Payment::sample("acme", "order-1");
        store.save(&acme).unwrap();

        let globex = TenantId::parse("globex").unwrap();
        let find = |tenant| store.find_by_idempotency_key(tenant, acme.idempotency_key());

        assert_eq!(find(acme.tenant()).unwrap(), Some(acme.clone()));
        assert_eq!(find(&globex).unwrap(), None);
        assert_eq!(store.get(&globex, acme.id()).unwrap(), None);
    }

    #[test]
    fn a_torn_final_line_is_discarded_on_open() {
        let path = log_path("torn");
        let payment = Payment::sample("acme", "order-1");
        FileStore::open(&path).unwrap().save(&payment).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{\"id\":\"pay_").unwrap();

        let store = FileStore::open(&path).unwrap();
        store.save(&payment).unwrap();

        assert_eq!(
            FileStore::open(&path)
                .unwrap()
                .get(payment.tenant(), payment.id())
                .unwrap(),
            Some(payment)
        );
    }
}
