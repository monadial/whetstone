//! Charges payments through the external payment provider.
//!
//! Each attempt has its own timeout, and a failed attempt is retried with
//! exponential backoff. Every attempt of a charge carries the same operation
//! key, so the provider applies the charge at most once.

use std::future::Future;
use std::num::NonZeroU32;
use std::time::Duration;

use reqwest::StatusCode;
use serde::Deserialize;

use crate::payment::ProviderOutcome;
use crate::ports::{ChargeRequest, PaymentProvider};

/// Why a single attempt failed.
#[derive(Debug, thiserror::Error)]
pub enum AttemptError {
    /// The provider was unreachable, overloaded or broken. Retrying may help.
    #[error("{0}")]
    Retryable(String),
    /// The provider refused the request itself, so retrying cannot help.
    #[error("{0}")]
    Rejected(String),
}

/// Sends one attempt of a charge to the provider.
pub trait Transport: Send + Sync {
    fn send(
        &self,
        request: &ChargeRequest,
    ) -> impl Future<Output = Result<ProviderOutcome, AttemptError>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub attempt_timeout: Duration,
    pub max_attempts: NonZeroU32,
    /// The wait before the first retry. It doubles before each later one.
    pub backoff: Duration,
}

#[derive(Debug)]
pub struct ProviderClient<T> {
    transport: T,
    policy: RetryPolicy,
}

impl<T: Transport> ProviderClient<T> {
    pub fn new(transport: T, policy: RetryPolicy) -> Self {
        Self { transport, policy }
    }
}

impl<T: Transport> PaymentProvider for ProviderClient<T> {
    /// Charges the request, retrying failed attempts.
    ///
    /// When no attempt gets an answer, the charge may still have happened,
    /// so the outcome is `Unknown` rather than declined.
    #[tracing::instrument(skip_all, fields(operation_key = request.operation_key.as_str()))]
    async fn charge(&self, request: &ChargeRequest) -> ProviderOutcome {
        let mut backoff = self.policy.backoff;
        for attempt in 1..=self.policy.max_attempts.get() {
            let sent =
                tokio::time::timeout(self.policy.attempt_timeout, self.transport.send(request));
            let failure = match sent.await {
                Ok(Ok(outcome)) => return outcome,
                Ok(Err(AttemptError::Rejected(reason))) => {
                    tracing::warn!(%reason, "provider rejected the charge");
                    return ProviderOutcome::Declined { reason };
                }
                Ok(Err(AttemptError::Retryable(reason))) => reason,
                Err(_) => "attempt timed out".to_owned(),
            };
            tracing::warn!(attempt, %failure, "charge attempt failed");
            if attempt < self.policy.max_attempts.get() {
                tokio::time::sleep(backoff).await;
                backoff = backoff.saturating_mul(2);
            }
        }
        tracing::error!("no charge attempt got an answer, outcome unknown");
        ProviderOutcome::Unknown
    }
}

/// Talks to the provider's HTTP API.
#[derive(Debug)]
pub struct HttpTransport {
    client: reqwest::Client,
    charges_url: String,
}

impl HttpTransport {
    pub fn new(base_url: &str) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::Client::builder().build()?,
            charges_url: format!("{}/v1/charges", base_url.trim_end_matches('/')),
        })
    }
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ChargeResponse {
    Succeeded { id: String },
    Declined { reason: String },
}

impl Transport for HttpTransport {
    async fn send(&self, request: &ChargeRequest) -> Result<ProviderOutcome, AttemptError> {
        let response = self
            .client
            .post(&self.charges_url)
            .header("Idempotency-Key", request.operation_key.as_str())
            .json(request)
            .send()
            .await
            .map_err(|error| AttemptError::Retryable(error.to_string()))?;
        let status = response.status();
        if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
            return Err(AttemptError::Retryable(format!(
                "provider answered {status}"
            )));
        }
        if !status.is_success() {
            return Err(AttemptError::Rejected(format!(
                "provider answered {status}"
            )));
        }
        let body: ChargeResponse = response
            .json()
            .await
            .map_err(|error| AttemptError::Retryable(error.to_string()))?;
        Ok(match body {
            ChargeResponse::Succeeded { id } => ProviderOutcome::Succeeded { provider_ref: id },
            ChargeResponse::Declined { reason } => ProviderOutcome::Declined { reason },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;
    use crate::payment::{OperationKey, Payment};

    enum Reply {
        Hang,
        Reject,
        Answer(ProviderOutcome),
    }

    /// A provider that answers each attempt from a script and remembers the
    /// operation keys it was sent.
    struct Scripted {
        replies: Mutex<VecDeque<Reply>>,
        keys: Mutex<Vec<OperationKey>>,
    }

    impl Scripted {
        fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
            Self {
                replies: Mutex::new(replies.into_iter().collect()),
                keys: Mutex::default(),
            }
        }
    }

    impl Transport for &Scripted {
        async fn send(&self, request: &ChargeRequest) -> Result<ProviderOutcome, AttemptError> {
            self.keys
                .lock()
                .unwrap()
                .push(request.operation_key.clone());
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Reply::Hang);
            match reply {
                Reply::Hang => std::future::pending().await,
                Reply::Reject => Err(AttemptError::Rejected("400 Bad Request".into())),
                Reply::Answer(outcome) => Ok(outcome),
            }
        }
    }

    fn request() -> ChargeRequest {
        ChargeRequest::new(&Payment::sample("acme", "order-7"))
    }

    fn client(transport: &Scripted) -> ProviderClient<&Scripted> {
        let policy = RetryPolicy {
            attempt_timeout: Duration::from_secs(2),
            max_attempts: NonZeroU32::new(3).unwrap(),
            backoff: Duration::from_millis(100),
        };
        ProviderClient::new(transport, policy)
    }

    #[tokio::test(start_paused = true)]
    async fn a_timed_out_attempt_is_retried_with_the_same_operation_key() {
        let succeeded = ProviderOutcome::Succeeded {
            provider_ref: "ch_1".into(),
        };
        let provider = Scripted::new([Reply::Hang, Reply::Answer(succeeded.clone())]);
        let request = request();

        let outcome = client(&provider).charge(&request).await;

        assert_eq!(outcome, succeeded);
        assert_eq!(
            *provider.keys.lock().unwrap(),
            [request.operation_key.clone(), request.operation_key]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_rejected_charge_is_declined_without_a_retry() {
        let provider = Scripted::new([Reply::Reject]);

        let outcome = client(&provider).charge(&request()).await;

        assert_eq!(
            outcome,
            ProviderOutcome::Declined {
                reason: "400 Bad Request".into()
            }
        );
        assert_eq!(provider.keys.lock().unwrap().len(), 1);
    }
}
