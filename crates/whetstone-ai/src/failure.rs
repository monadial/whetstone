//! Why a call to Jev or the generator failed, named for the learner.

use rig::error::{ErrorKind, ErrorReport, ProviderError};
use serde_json::Value;

/// Why a call failed. Nothing is retried automatically (rule 23): retrying is
/// the learner's choice.
///
/// No variant carries reply text, so a failure can be logged.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AiError {
    /// There is no `OpenRouter` key: `OPENROUTER_API_KEY` is unset or blank.
    #[error("no OpenRouter key: OPENROUTER_API_KEY is not set")]
    NoKey,
    /// `OpenRouter` refused the key (401).
    #[error("the OpenRouter key was refused")]
    KeyRefused,
    /// The key's account is out of credits (402).
    #[error("out of OpenRouter credits")]
    OutOfCredits,
    /// `OpenRouter` is rate limiting the key (429).
    #[error("rate limited by OpenRouter")]
    RateLimited,
    /// `OpenRouter` or the model's provider failed (5xx), or `OpenRouter`
    /// couldn't be reached at all (`status` is `None`).
    #[error("the model's provider or OpenRouter is unavailable")]
    ProviderUnavailable {
        /// The reply's status, or `None` when nothing replied.
        status: Option<u16>,
    },
    /// `OpenRouter` refused the route: no endpoint for `model` meets the
    /// request's routing, which for the generator is zero-data retention
    /// with data collection denied.
    #[error("no zero-retention endpoint for {model} (refused by OpenRouter)")]
    NoZeroRetentionEndpoint {
        /// The model that was asked for.
        model: String,
    },
    /// The call took longer than its time limit, or `OpenRouter` gave up
    /// waiting for the provider (408).
    #[error("no reply in time")]
    TimedOut,
    /// The reply came but couldn't be read: an answer is missing, unexpected
    /// or out of range, or the output doesn't decode into the requested
    /// type. It is never coerced (rule 37).
    #[error("the reply could not be read")]
    InvalidReply,
    /// The request was invalid: Rig couldn't build it (`status` is `None`),
    /// or `OpenRouter` refused it with another 4xx status: 400 for a model it
    /// doesn't know, say, or 403 when its moderation blocks the input.
    #[error("the request was invalid")]
    InvalidRequest {
        /// `OpenRouter`'s status, or `None` when nothing was sent.
        status: Option<u16>,
    },
    /// The call's job was cancelled before a reply came.
    #[error("cancelled")]
    Cancelled,
}

impl AiError {
    /// The failure a Rig provider error stands for, for a call to `model`.
    pub(crate) fn from_provider(error: &ProviderError, model: &str) -> Self {
        Self::classify(
            error
                .provider_response_status()
                .map(|status| status.as_u16()),
            error.provider_response_json().ok().flatten().as_ref(),
            error.kind(),
            model,
        )
    }

    /// The failure a Rig error report stands for, for a call to `model`.
    pub(crate) fn from_report(report: &ErrorReport, model: &str) -> Self {
        Self::classify(
            report.http_status,
            report.provider_response_json().ok().flatten().as_ref(),
            report.kind,
            model,
        )
    }

    fn classify(status: Option<u16>, body: Option<&Value>, kind: ErrorKind, model: &str) -> Self {
        if let Some(status) = status {
            if body.is_some_and(refuses_route) {
                return Self::NoZeroRetentionEndpoint {
                    model: model.to_owned(),
                };
            }
            return match status {
                401 => Self::KeyRefused,
                402 => Self::OutOfCredits,
                408 => Self::TimedOut,
                429 => Self::RateLimited,
                500..=599 => Self::ProviderUnavailable {
                    status: Some(status),
                },
                _ => Self::InvalidRequest {
                    status: Some(status),
                },
            };
        }
        match kind {
            // A transport failure with no reply: OpenRouter couldn't be
            // reached, or the provider gave up without a status.
            ErrorKind::Http | ErrorKind::Provider | ErrorKind::ProviderResponse => {
                Self::ProviderUnavailable { status: None }
            }
            ErrorKind::Json | ErrorKind::Response => Self::InvalidReply,
            ErrorKind::Timeout => Self::TimedOut,
            ErrorKind::Cancelled => Self::Cancelled,
            // Rig refused to build the request, or one of its own parts
            // failed, which no request of ours should cause.
            _ => Self::InvalidRequest { status: None },
        }
    }
}

/// Whether an error reply is `OpenRouter` refusing the route: its error names
/// the routing step that left no endpoint, as the recorded
/// `generator-no-zero-retention.json` names "Filter by Data Policy". A
/// request's only routing is its data policy, so any refused step means no
/// endpoint keeps to it.
fn refuses_route(body: &Value) -> bool {
    body.pointer("/error/metadata/failed_routing_step")
        .is_some_and(Value::is_string)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::Instant;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::replay::{
        GENERATOR_MODEL, JEV_MODEL, SampleQuestion, connection, sample_generator_request,
        sample_jev_request, server,
    };
    use crate::{GENERATOR_TIMEOUT, JEV_TIMEOUT, OpenRouter, SecretString, Timeouts};

    /// Timeouts whose connect limit can't fire first, for the paused-clock
    /// tests: a paused clock jumps to the next timer whenever the runtime
    /// waits on the socket.
    const NO_CONNECT_LIMIT: Timeouts = Timeouts {
        connect: Duration::from_secs(3600),
        jev: JEV_TIMEOUT,
        generator: GENERATOR_TIMEOUT,
    };

    /// How a Jev call and a generator call fail when the server replies with
    /// `status` and `body` after `delay`, cancelled by `cancel`.
    async fn failures_with(
        status: u16,
        body: &str,
        delay: Duration,
        timeouts: Timeouts,
        cancel: &CancellationToken,
    ) -> (AiError, AiError) {
        let jev = server("/systemone", status, body, delay).await;
        let jev = connection(&jev, timeouts)
            .evaluate(&sample_jev_request(), cancel)
            .await
            .expect_err("the Jev call fails");
        let generator = server("/chat/completions", status, body, delay).await;
        let generator = connection(&generator, timeouts)
            .generate::<SampleQuestion>(&sample_generator_request(), cancel)
            .await
            .expect_err("the generator call fails");
        (jev, generator)
    }

    async fn failures(status: u16, body: &str) -> (AiError, AiError) {
        failures_with(
            status,
            body,
            Duration::ZERO,
            Timeouts::default(),
            &CancellationToken::new(),
        )
        .await
    }

    // Bodies that weren't recorded live follow `OpenRouter`'s documented error
    // shape, `{"error": {"code", "message", "metadata"?}}`:
    // https://openrouter.ai/docs/api_reference/errors-and-debugging

    #[tokio::test]
    async fn a_refused_key_is_key_refused() {
        let refused = include_str!("../fixtures/key-refused.json");

        assert_eq!(
            failures(401, refused).await,
            (AiError::KeyRefused, AiError::KeyRefused)
        );
    }

    #[tokio::test]
    async fn no_credits_is_out_of_credits() {
        let body = r#"{"error":{"code":402,"message":"Insufficient credits. Add more using https://openrouter.ai/settings/credits"}}"#;

        assert_eq!(
            failures(402, body).await,
            (AiError::OutOfCredits, AiError::OutOfCredits)
        );
    }

    #[tokio::test]
    async fn too_many_requests_is_rate_limited() {
        let body = r#"{"error":{"code":429,"message":"Rate limit exceeded","metadata":{"error_type":"rate_limit_exceeded"}}}"#;

        assert_eq!(
            failures(429, body).await,
            (AiError::RateLimited, AiError::RateLimited)
        );
    }

    #[tokio::test]
    async fn a_server_error_is_provider_unavailable() {
        for status in [500, 502, 503] {
            let body =
                format!(r#"{{"error":{{"code":{status},"message":"Internal Server Error"}}}}"#);
            let unavailable = AiError::ProviderUnavailable {
                status: Some(status),
            };

            assert_eq!(
                failures(status, &body).await,
                (unavailable.clone(), unavailable),
                "status {status}"
            );
        }
    }

    #[tokio::test]
    async fn an_error_reported_after_the_reply_started_is_named_by_its_code() {
        // Once the model starts, `OpenRouter` replies 200 and reports a later
        // failure in the body. Rig's chat wire reads the error's code;
        // `rig-typesafeai` 0.45 reads any 200 as answers, so for Jev the
        // reply is one that can't be read.
        let body = r#"{"error":{"code":502,"message":"Provider returned error"}}"#;

        assert_eq!(
            failures(200, body).await,
            (
                AiError::InvalidReply,
                AiError::ProviderUnavailable { status: Some(502) }
            )
        );
    }

    #[tokio::test]
    async fn a_refused_route_is_no_zero_retention_endpoint_for_the_model() {
        let refused = include_str!("../fixtures/generator-no-zero-retention.json");

        assert_eq!(
            failures(404, refused).await,
            (
                AiError::NoZeroRetentionEndpoint {
                    model: JEV_MODEL.to_owned()
                },
                AiError::NoZeroRetentionEndpoint {
                    model: GENERATOR_MODEL.to_owned()
                },
            )
        );
    }

    #[tokio::test]
    async fn another_refused_request_is_an_invalid_request() {
        let body = r#"{"error":{"code":400,"message":"anthropic/claude-sonnet-9 is not a valid model ID"}}"#;
        let invalid = AiError::InvalidRequest { status: Some(400) };

        assert_eq!(failures(400, body).await, (invalid.clone(), invalid));
    }

    #[tokio::test]
    async fn openrouter_giving_up_on_the_provider_is_timed_out() {
        let body = r#"{"error":{"code":408,"message":"Request timed out"}}"#;

        assert_eq!(
            failures(408, body).await,
            (AiError::TimedOut, AiError::TimedOut)
        );
    }

    #[tokio::test]
    async fn an_undecodable_reply_is_an_invalid_reply() {
        assert_eq!(
            failures(200, "not json").await,
            (AiError::InvalidReply, AiError::InvalidReply)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_jev_call_times_out_after_30_seconds() {
        let server = server("/systemone", 200, "{}", Duration::from_secs(600)).await;
        let started = Instant::now();

        let reply = connection(&server, NO_CONNECT_LIMIT)
            .evaluate(&sample_jev_request(), &CancellationToken::new())
            .await;

        assert_eq!(reply, Err(AiError::TimedOut));
        assert_eq!(started.elapsed(), Duration::from_secs(30));
    }

    #[tokio::test(start_paused = true)]
    async fn a_generator_call_times_out_after_120_seconds() {
        let server = server("/chat/completions", 200, "{}", Duration::from_secs(600)).await;
        let started = Instant::now();

        let reply = connection(&server, NO_CONNECT_LIMIT)
            .generate::<SampleQuestion>(&sample_generator_request(), &CancellationToken::new())
            .await;

        assert_eq!(reply.err(), Some(AiError::TimedOut));
        assert_eq!(started.elapsed(), Duration::from_secs(120));
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_a_call_ends_it_as_cancelled() {
        let cancel = CancellationToken::new();
        let cancelling = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            cancelling.cancel();
        });

        let failures = failures_with(
            200,
            "{}",
            Duration::from_secs(600),
            NO_CONNECT_LIMIT,
            &cancel,
        )
        .await;

        assert_eq!(failures, (AiError::Cancelled, AiError::Cancelled));
    }

    #[tokio::test]
    async fn nothing_listening_is_provider_unavailable() {
        // A port that was free a moment ago, so nothing answers on it.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .expect("a free port")
            .port();
        let connection = OpenRouter::with_settings(
            Some(SecretString::from("sk-or-v1-test")),
            format!("http://127.0.0.1:{port}/api/v1"),
            Timeouts::default(),
        )
        .expect("the key isn't blank");

        let reply = connection
            .evaluate(&sample_jev_request(), &CancellationToken::new())
            .await;

        assert_eq!(reply, Err(AiError::ProviderUnavailable { status: None }));
    }

    #[test]
    fn no_key_or_a_blank_one_is_no_key() {
        assert_eq!(OpenRouter::new(None).err(), Some(AiError::NoKey));
        assert_eq!(
            OpenRouter::new(Some(SecretString::from(" \t"))).err(),
            Some(AiError::NoKey)
        );
    }
}
