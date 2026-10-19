//! The payment use cases behind the HTTP API: paying and looking payments up.

use crate::cache::TenantCache;
use crate::payment::{IdempotencyKey, Money, Payment, PaymentError, PaymentId, TenantId};
use crate::ports::{ChargeRequest, PaymentProvider, PaymentStore, StoreError};
use crate::telemetry::Metrics;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Invalid(#[from] PaymentError),
    #[error("idempotency key is already used by a different payment")]
    KeyReused,
    #[error("payment not found")]
    NotFound,
    #[error(transparent)]
    Store(#[from] StoreError),
}

#[derive(Debug)]
pub struct PaymentService<S, P> {
    store: S,
    provider: P,
    cache: TenantCache,
    metrics: Metrics,
}

impl<S: PaymentStore, P: PaymentProvider> PaymentService<S, P> {
    pub fn new(store: S, provider: P, cache: TenantCache) -> Self {
        Self {
            store,
            provider,
            cache,
            metrics: Metrics::default(),
        }
    }

    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// Creates a payment and charges it.
    ///
    /// Repeating a request with the same idempotency key returns the same
    /// payment. It is charged again only while its outcome is still unknown.
    #[tracing::instrument(skip_all, fields(tenant = %tenant))]
    pub async fn pay(
        &self,
        tenant: TenantId,
        idempotency_key: IdempotencyKey,
        amount: Money,
    ) -> Result<Payment, ServiceError> {
        let mut payment = match self
            .store
            .find_by_idempotency_key(&tenant, &idempotency_key)?
        {
            Some(existing) if existing.amount() != &amount => return Err(ServiceError::KeyReused),
            Some(existing) if existing.is_settled() => return Ok(existing),
            Some(existing) => existing,
            None => {
                let payment = Payment::new(tenant, idempotency_key, amount);
                // Saved before charging, so a crash mid-charge leaves a record to retry.
                self.save(&payment).map_err(|error| match error {
                    StoreError::DuplicateKey => ServiceError::KeyReused,
                    other => ServiceError::Store(other),
                })?;
                self.metrics.payment_created();
                payment
            }
        };
        let outcome = self.provider.charge(&ChargeRequest::new(&payment)).await;
        payment.record(outcome)?;
        self.save(&payment)?;
        self.metrics.charge_finished(payment.status());
        tracing::info!(payment = %payment.id(), status = ?payment.status(), "payment charged");
        Ok(payment)
    }

    pub fn get(&self, tenant: &TenantId, id: &PaymentId) -> Result<Payment, ServiceError> {
        if let Some(payment) = self.cache.get(tenant, id) {
            return Ok(payment);
        }
        let payment = self.store.get(tenant, id)?.ok_or(ServiceError::NotFound)?;
        self.cache.fill(&payment);
        Ok(payment)
    }

    /// Saves through to the cache, so a cached payment is never older than
    /// the stored one.
    fn save(&self, payment: &Payment) -> Result<(), StoreError> {
        self.store.save(payment)?;
        self.cache.update(payment);
        Ok(())
    }
}
