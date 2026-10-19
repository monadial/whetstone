//! Keeps recent payments in memory, with a separate bounded space for each
//! tenant so that one busy tenant cannot evict another tenant's payments.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard};

use crate::payment::{Payment, PaymentId, TenantId};

#[derive(Debug)]
pub struct TenantCache {
    capacity_per_tenant: usize,
    tenants: Mutex<HashMap<TenantId, Entries>>,
}

#[derive(Debug, Default)]
struct Entries {
    /// Oldest first: the front is evicted next.
    order: VecDeque<PaymentId>,
    payments: HashMap<PaymentId, Payment>,
}

impl TenantCache {
    pub fn new(capacity_per_tenant: usize) -> Self {
        Self {
            capacity_per_tenant,
            tenants: Mutex::default(),
        }
    }

    pub fn get(&self, tenant: &TenantId, id: &PaymentId) -> Option<Payment> {
        self.lock().get(tenant)?.payments.get(id).cloned()
    }

    /// Caches a payment that was just saved, replacing any older copy.
    pub fn update(&self, payment: &Payment) {
        self.insert(payment, true);
    }

    /// Caches a payment read from storage, unless a copy is already cached.
    /// That copy may come from a save that raced this read, so it wins.
    pub fn fill(&self, payment: &Payment) {
        self.insert(payment, false);
    }

    fn insert(&self, payment: &Payment, replace: bool) {
        let mut tenants = self.lock();
        let entries = tenants.entry(payment.tenant().clone()).or_default();
        if let Some(cached) = entries.payments.get_mut(payment.id()) {
            if replace {
                cached.clone_from(payment);
            }
            return;
        }
        let id = payment.id().clone();
        entries.payments.insert(id.clone(), payment.clone());
        entries.order.push_back(id);
        if entries.order.len() > self.capacity_per_tenant
            && let Some(oldest) = entries.order.pop_front()
        {
            entries.payments.remove(&oldest);
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<TenantId, Entries>> {
        // A panic mid-update costs at most one entry that is never evicted,
        // so keep serving rather than fail every later request.
        self.tenants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payment::ProviderOutcome;

    #[test]
    fn a_busy_tenant_cannot_evict_another_tenants_payments() {
        let cache = TenantCache::new(2);
        let quiet = Payment::sample("quiet", "order");
        cache.update(&quiet);
        let busy: Vec<_> = (0..3).map(|_| Payment::sample("busy", "order")).collect();
        busy.iter().for_each(|payment| cache.update(payment));

        assert_eq!(cache.get(quiet.tenant(), quiet.id()), Some(quiet));
        assert_eq!(cache.get(busy[0].tenant(), busy[0].id()), None);
        assert_eq!(
            cache.get(busy[2].tenant(), busy[2].id()),
            Some(busy[2].clone())
        );
    }

    #[test]
    fn a_read_never_replaces_a_newer_cached_copy() {
        let cache = TenantCache::new(2);
        let stale = Payment::sample("acme", "order");
        let mut fresh = stale.clone();
        fresh.record(ProviderOutcome::Unknown).unwrap();

        cache.update(&fresh);
        cache.fill(&stale);

        assert_eq!(cache.get(fresh.tenant(), fresh.id()), Some(fresh));
    }
}
