//! The boundaries between the payment service and the outside world.
//!
//! The service depends only on these traits. The file store and the
//! provider client implement them, and `main` decides which to plug in.

use std::future::Future;
use std::io;

use serde::Serialize;

use crate::payment::{IdempotencyKey, OperationKey, Payment, PaymentId, ProviderOutcome, TenantId};

/// Keeps payments.
pub trait PaymentStore: Send + Sync {
    /// Saves the payment's current state. Saving a second payment under an
    /// idempotency key that is already taken fails with `DuplicateKey`.
    fn save(&self, payment: &Payment) -> Result<(), StoreError>;

    fn get(&self, tenant: &TenantId, id: &PaymentId) -> Result<Option<Payment>, StoreError>;

    fn find_by_idempotency_key(
        &self,
        tenant: &TenantId,
        key: &IdempotencyKey,
    ) -> Result<Option<Payment>, StoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("payment storage i/o failed")]
    Io(#[from] io::Error),
    #[error("stored payment at byte {offset} is corrupt")]
    Corrupt {
        offset: u64,
        source: serde_json::Error,
    },
    #[error("idempotency key is already used by another payment")]
    DuplicateKey,
}

/// Charges payments.
pub trait PaymentProvider: Send + Sync {
    /// Charges the request. It never fails outright: a charge that got no
    /// answer is `ProviderOutcome::Unknown`.
    fn charge(&self, request: &ChargeRequest) -> impl Future<Output = ProviderOutcome> + Send;
}

/// One charge, as the provider sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChargeRequest {
    #[serde(skip)]
    pub operation_key: OperationKey,
    pub reference: String,
    pub amount_minor: u64,
    pub currency: String,
}

impl ChargeRequest {
    /// Builds the charge for a payment. It always carries the operation key
    /// the payment was created with, so every retry is the same charge.
    pub fn new(payment: &Payment) -> Self {
        Self {
            operation_key: payment.operation_key().clone(),
            reference: payment.id().to_string(),
            amount_minor: payment.amount().amount_minor(),
            currency: payment.amount().currency().as_str().to_owned(),
        }
    }
}
