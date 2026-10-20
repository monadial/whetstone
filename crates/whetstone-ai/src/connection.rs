//! The `OpenRouter` connection: the key, the base URL and the time limits that
//! every call to Jev and the generator goes out with.

use std::future::Future;
use std::time::Duration;

use rig::http_client::ReqwestClient;
use rig::rig_reqwest::reqwest;
use secrecy::{ExposeSecret, SecretString};
use tokio_util::sync::CancellationToken;

use crate::failure::AiError;

/// Where `OpenRouter`'s API lives. Calls go to `/systemone` (Jev) and
/// `/chat/completions` (the generator) under it.
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// How long a call waits to connect to `OpenRouter` (rule 22).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long one Jev call may take, connecting included (rule 22).
pub const JEV_TIMEOUT: Duration = Duration::from_secs(30);

/// How long one generator call may take, connecting included (rule 22).
pub const GENERATOR_TIMEOUT: Duration = Duration::from_secs(120);

/// The HTTP client's own timeout is a backstop this far past a call's time
/// limit, so that the call's limit is what fires and reports
/// [`AiError::TimedOut`].
const CLIENT_TIMEOUT_GRACE: Duration = Duration::from_secs(1);

/// The time limits calls go out with. [`Default`] gives the constants above;
/// tests inject shorter ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// How long a call waits to connect.
    pub connect: Duration,
    /// How long one Jev call may take.
    pub jev: Duration,
    /// How long one generator call may take.
    pub generator: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: CONNECT_TIMEOUT,
            jev: JEV_TIMEOUT,
            generator: GENERATOR_TIMEOUT,
        }
    }
}

/// A connection to `OpenRouter`: the key, the base URL, the time limits and an
/// HTTP client for each kind of call. Cloning it shares the clients.
///
/// Its `Debug` output redacts the key (rule 52).
#[derive(Debug, Clone)]
pub struct OpenRouter {
    key: SecretString,
    base_url: String,
    timeouts: Timeouts,
    jev_http: ReqwestClient,
    generator_http: ReqwestClient,
}

impl OpenRouter {
    /// A connection to `OpenRouter`'s API with `key`, at the default time
    /// limits.
    ///
    /// Fails with [`AiError::NoKey`] when there is no key, or it is blank.
    pub fn new(key: Option<SecretString>) -> Result<Self, AiError> {
        Self::with_settings(key, OPENROUTER_BASE_URL, Timeouts::default())
    }

    /// A connection to the API at `base_url` (such as a test's local
    /// server), with `timeouts`.
    ///
    /// Fails with [`AiError::NoKey`] when there is no key, or it is blank,
    /// and with [`AiError::ProviderUnavailable`] when the HTTP client can't
    /// be built.
    pub fn with_settings(
        key: Option<SecretString>,
        base_url: impl Into<String>,
        timeouts: Timeouts,
    ) -> Result<Self, AiError> {
        // A blank key is as good as none. Checking it sends and logs nothing.
        let key = key
            .filter(|key| !key.expose_secret().trim().is_empty())
            .ok_or(AiError::NoKey)?;
        let http = |limit| {
            reqwest::Client::builder()
                .connect_timeout(timeouts.connect)
                .timeout(limit + CLIENT_TIMEOUT_GRACE)
                .build()
                .map(ReqwestClient::from)
                .map_err(|_| AiError::ProviderUnavailable { status: None })
        };
        Ok(Self {
            key,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            timeouts,
            jev_http: http(timeouts.jev)?,
            generator_http: http(timeouts.generator)?,
        })
    }

    /// The key, for Rig to build the auth header from. Nothing else reads
    /// it (rule 41).
    pub(crate) fn key_for_auth_header(&self) -> String {
        self.key.expose_secret().to_owned()
    }

    /// The URL `path` resolves to under the base URL.
    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn timeouts(&self) -> Timeouts {
        self.timeouts
    }

    pub(crate) fn jev_http(&self) -> ReqwestClient {
        self.jev_http.clone()
    }

    pub(crate) fn generator_http(&self) -> ReqwestClient {
        self.generator_http.clone()
    }
}

/// Runs `call` within `limit`, ending it early when `cancel` is cancelled.
/// Dropping the call drops its request.
pub(crate) async fn bounded<T>(
    limit: Duration,
    cancel: &CancellationToken,
    call: impl Future<Output = Result<T, AiError>>,
) -> Result<T, AiError> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(AiError::Cancelled),
        reply = tokio::time::timeout(limit, call) => reply.unwrap_or(Err(AiError::TimedOut)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connection_prints_without_its_key() {
        let connection = OpenRouter::new(Some(SecretString::from("sk-or-v1-secret")))
            .expect("the key isn't blank");

        assert!(!format!("{connection:?}").contains("sk-or-v1-secret"));
    }

    /// Calls can run on a multi-threaded runtime's tasks and in parallel, as
    /// background jobs do.
    #[test]
    fn calls_can_be_spawned() {
        fn spawnable(_: impl Future + Send + 'static) {}
        let connection = OpenRouter::new(Some(SecretString::from("sk-or-v1-test")))
            .expect("the key isn't blank");
        let cancel = CancellationToken::new();

        let jev = connection.clone();
        let jev_cancel = cancel.clone();
        spawnable(async move {
            jev.evaluate(&crate::replay::sample_jev_request(), &jev_cancel)
                .await
        });
        spawnable(async move {
            connection
                .generate::<crate::replay::SampleQuestion>(
                    &crate::replay::sample_generator_request(),
                    &cancel,
                )
                .await
        });
    }
}
