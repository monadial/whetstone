//! The payment lifecycle: what a payment is, the states it moves through and
//! the transitions between them.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Why a request or a transition was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaymentError {
    #[error("tenant id must be 1-64 lowercase letters, digits or dashes, got {0:?}")]
    InvalidTenant(String),
    #[error("idempotency key must be 1-255 visible ASCII characters")]
    InvalidIdempotencyKey,
    #[error("currency must be a three-letter ISO 4217 code, got {0:?}")]
    InvalidCurrency(String),
    #[error("amount must be greater than zero")]
    ZeroAmount,
    #[error("payment {0} is already settled")]
    AlreadySettled(PaymentId),
}

/// A merchant using the service. Every payment belongs to exactly one tenant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(String);

impl TenantId {
    pub fn parse(raw: &str) -> Result<Self, PaymentError> {
        let allowed = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
        if (1..=64).contains(&raw.len()) && raw.bytes().all(allowed) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(PaymentError::InvalidTenant(raw.to_owned()))
        }
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The key a client sends so that repeating a request never creates a second
/// payment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn parse(raw: &str) -> Result<Self, PaymentError> {
        if (1..=255).contains(&raw.len()) && raw.bytes().all(|b| b.is_ascii_graphic()) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(PaymentError::InvalidIdempotencyKey)
        }
    }
}

/// Identifies a payment within its tenant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaymentId(String);

impl PaymentId {
    pub fn generate() -> Self {
        Self(format!("pay_{}", unique_token()))
    }

    /// Wraps an id from a request. An id that was never issued simply
    /// matches no payment.
    pub fn from_raw(raw: String) -> Self {
        Self(raw)
    }
}

impl fmt::Display for PaymentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The key the provider deduplicates charges on. Two submissions with the
/// same operation key are the same charge, so the provider applies it at most
/// once.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationKey(String);

impl OperationKey {
    pub fn generate() -> Self {
        Self(format!("op_{}", unique_token()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A unique, roughly time-ordered token for identifiers.
fn unique_token() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}{sequence:04x}")
}

/// An ISO 4217 currency code such as `EUR`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Currency(String);

impl Currency {
    pub fn parse(code: &str) -> Result<Self, PaymentError> {
        if code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()) {
            Ok(Self(code.to_owned()))
        } else {
            Err(PaymentError::InvalidCurrency(code.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An amount in the currency's minor unit, such as cents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money {
    amount_minor: u64,
    currency: Currency,
}

impl Money {
    pub fn new(amount_minor: u64, currency: Currency) -> Result<Self, PaymentError> {
        if amount_minor == 0 {
            return Err(PaymentError::ZeroAmount);
        }
        Ok(Self {
            amount_minor,
            currency,
        })
    }

    pub fn amount_minor(&self) -> u64 {
        self.amount_minor
    }

    pub fn currency(&self) -> &Currency {
        &self.currency
    }
}

/// The provider's answer to one charge submission. `Unknown` means the
/// provider did not answer, so the charge may or may not have happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderOutcome {
    Succeeded { provider_ref: String },
    Declined { reason: String },
    Unknown,
}

/// Where a payment is in its lifecycle.
///
/// A payment starts `Pending`. `Unknown` means the last submission got no
/// answer; submitting again with the same operation key is safe, because the
/// provider charges at most once. `Succeeded` and `Declined` are settled and
/// never change again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentStatus {
    Pending,
    Succeeded { provider_ref: String },
    Declined { reason: String },
    Unknown,
}

/// A tenant's request to charge an amount, and what has happened to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payment {
    id: PaymentId,
    tenant: TenantId,
    idempotency_key: IdempotencyKey,
    operation_key: OperationKey,
    amount: Money,
    status: PaymentStatus,
}

impl Payment {
    pub fn new(tenant: TenantId, idempotency_key: IdempotencyKey, amount: Money) -> Self {
        Self {
            id: PaymentId::generate(),
            tenant,
            idempotency_key,
            operation_key: OperationKey::generate(),
            amount,
            status: PaymentStatus::Pending,
        }
    }

    pub fn id(&self) -> &PaymentId {
        &self.id
    }

    pub fn tenant(&self) -> &TenantId {
        &self.tenant
    }

    pub fn idempotency_key(&self) -> &IdempotencyKey {
        &self.idempotency_key
    }

    pub fn operation_key(&self) -> &OperationKey {
        &self.operation_key
    }

    pub fn amount(&self) -> &Money {
        &self.amount
    }

    pub fn status(&self) -> &PaymentStatus {
        &self.status
    }

    pub fn is_settled(&self) -> bool {
        matches!(
            self.status,
            PaymentStatus::Succeeded { .. } | PaymentStatus::Declined { .. }
        )
    }

    /// Records the outcome of one submission to the provider.
    pub fn record(&mut self, outcome: ProviderOutcome) -> Result<(), PaymentError> {
        if self.is_settled() {
            return Err(PaymentError::AlreadySettled(self.id.clone()));
        }
        self.status = match outcome {
            ProviderOutcome::Succeeded { provider_ref } => {
                PaymentStatus::Succeeded { provider_ref }
            }
            ProviderOutcome::Declined { reason } => PaymentStatus::Declined { reason },
            ProviderOutcome::Unknown => PaymentStatus::Unknown,
        };
        Ok(())
    }
}

#[cfg(test)]
impl Payment {
    /// A pending EUR 12.50 payment, for tests.
    pub(crate) fn sample(tenant: &str, key: &str) -> Self {
        let amount = Money::new(1250, Currency::parse("EUR").unwrap()).unwrap();
        Self::new(
            TenantId::parse(tenant).unwrap(),
            IdempotencyKey::parse(key).unwrap(),
            amount,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_outcome_can_be_retried_with_the_same_operation_key() {
        let mut payment = Payment::sample("acme", "order-42");
        let key = payment.operation_key().clone();
        assert_eq!(payment.status(), &PaymentStatus::Pending);

        payment.record(ProviderOutcome::Unknown).unwrap();
        assert_eq!(payment.status(), &PaymentStatus::Unknown);
        payment
            .record(ProviderOutcome::Succeeded {
                provider_ref: "ch_1".into(),
            })
            .unwrap();

        assert_eq!(
            payment.status(),
            &PaymentStatus::Succeeded {
                provider_ref: "ch_1".into()
            }
        );
        assert_eq!(payment.operation_key(), &key);
    }

    #[test]
    fn a_settled_payment_rejects_another_outcome() {
        let mut payment = Payment::sample("acme", "order-42");
        payment
            .record(ProviderOutcome::Declined {
                reason: "insufficient_funds".into(),
            })
            .unwrap();

        let result = payment.record(ProviderOutcome::Succeeded {
            provider_ref: "ch_2".into(),
        });

        assert_eq!(
            result,
            Err(PaymentError::AlreadySettled(payment.id().clone()))
        );
    }

    #[test]
    fn requests_are_validated() {
        assert!(TenantId::parse("Acme Inc").is_err());
        assert!(TenantId::parse("").is_err());
        assert!(IdempotencyKey::parse("").is_err());
        assert!(Currency::parse("eur").is_err());
        assert_eq!(
            Money::new(0, Currency::parse("EUR").unwrap()),
            Err(PaymentError::ZeroAmount)
        );
    }
}
