//! Drives the HTTP API end to end against a fake provider on localhost.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use payments::cache::TenantCache;
use payments::provider::{HttpTransport, ProviderClient, RetryPolicy};
use payments::service::PaymentService;
use payments::store::FileStore;
use serde_json::{Value, json};

/// Records the operation key of every charge and fails the first few.
#[derive(Clone, Default)]
struct FakeProvider {
    keys: Arc<Mutex<Vec<String>>>,
    failures: Arc<Mutex<usize>>,
}

async fn charge(State(fake): State<FakeProvider>, headers: HeaderMap) -> Response {
    let key = headers["idempotency-key"].to_str().unwrap().to_owned();
    fake.keys.lock().unwrap().push(key);
    let mut failures = fake.failures.lock().unwrap();
    if *failures > 0 {
        *failures -= 1;
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    axum::Json(json!({ "status": "succeeded", "id": "ch_1" })).into_response()
}

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{address}")
}

/// Starts the service with a fresh payment log and returns its base URL.
async fn start(fake: &FakeProvider, name: &str) -> String {
    let routes = Router::new().route("/v1/charges", post(charge));
    let provider = serve(routes.with_state(fake.clone())).await;
    let log = std::env::temp_dir().join(format!("payments-api-{}-{name}.log", std::process::id()));
    std::fs::remove_file(&log).ok();
    let policy = RetryPolicy {
        attempt_timeout: Duration::from_secs(1),
        max_attempts: 2.try_into().unwrap(),
        backoff: Duration::from_millis(1),
    };
    let client = ProviderClient::new(HttpTransport::new(&provider).unwrap(), policy);
    let service = PaymentService::new(FileStore::open(&log).unwrap(), client, TenantCache::new(8));
    serve(payments::http::router(Arc::new(service))).await
}

async fn pay(api: &str, tenant: &str, key: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{api}/v1/tenants/{tenant}/payments"))
        .header("Idempotency-Key", key)
        .json(&json!({ "amount_minor": 1250, "currency": "EUR" }))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn repeating_a_request_returns_the_same_payment_without_charging_again() {
    let fake = FakeProvider::default();
    let api = start(&fake, "repeat").await;

    let first: Value = pay(&api, "acme", "order-1").await.json().await.unwrap();
    let second: Value = pay(&api, "acme", "order-1").await.json().await.unwrap();

    assert_eq!(
        first["status"],
        json!({ "succeeded": { "provider_ref": "ch_1" } })
    );
    assert_eq!(second, first);
    assert_eq!(fake.keys.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_tenant_cannot_read_another_tenants_payment() {
    let api = start(&FakeProvider::default(), "tenants").await;
    let payment: Value = pay(&api, "acme", "order-1").await.json().await.unwrap();
    let id = payment["id"].as_str().unwrap();

    let own = reqwest::get(format!("{api}/v1/tenants/acme/payments/{id}"))
        .await
        .unwrap();
    let other = reqwest::get(format!("{api}/v1/tenants/globex/payments/{id}"))
        .await
        .unwrap();

    assert_eq!(own.status(), StatusCode::OK);
    assert_eq!(other.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_request_without_an_idempotency_key_is_rejected() {
    let api = start(&FakeProvider::default(), "no-key").await;

    let response = pay(&api, "acme", "").await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_retry_after_an_unknown_outcome_reuses_the_operation_key() {
    let fake = FakeProvider::default();
    *fake.failures.lock().unwrap() = 2;
    let api = start(&fake, "retry").await;

    let first: Value = pay(&api, "acme", "order-1").await.json().await.unwrap();
    let retried: Value = pay(&api, "acme", "order-1").await.json().await.unwrap();

    assert_eq!(first["status"], "unknown");
    assert_eq!(
        retried["status"],
        json!({ "succeeded": { "provider_ref": "ch_1" } })
    );
    let keys = fake.keys.lock().unwrap();
    assert_eq!(keys.len(), 3);
    assert!(
        keys.iter().all(|key| key == &keys[0]),
        "operation keys differ: {keys:?}"
    );
}

#[tokio::test]
async fn metrics_count_payments_and_charge_outcomes() {
    let api = start(&FakeProvider::default(), "metrics").await;
    pay(&api, "acme", "order-1").await;

    let metrics = reqwest::get(format!("{api}/metrics"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(metrics.contains("payments_created_total 1\n"), "{metrics}");
    assert!(
        metrics.contains("payments_charges_total{outcome=\"succeeded\"} 1\n"),
        "{metrics}"
    );
}
