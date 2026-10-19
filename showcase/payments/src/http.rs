//! The HTTP API.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::payment::{
    Currency, IdempotencyKey, Money, Payment, PaymentId, PaymentStatus, TenantId,
};
use crate::ports::{PaymentProvider, PaymentStore};
use crate::service::{PaymentService, ServiceError};

type Service<S, P> = State<Arc<PaymentService<S, P>>>;

pub fn router<S, P>(service: Arc<PaymentService<S, P>>) -> Router
where
    S: PaymentStore + 'static,
    P: PaymentProvider + 'static,
{
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/metrics", get(metrics::<S, P>))
        .route("/v1/tenants/{tenant}/payments", post(create::<S, P>))
        .route("/v1/tenants/{tenant}/payments/{id}", get(fetch::<S, P>))
        .with_state(service)
}

#[derive(Deserialize)]
struct CreatePayment {
    amount_minor: u64,
    currency: String,
}

/// A payment as clients see it.
#[derive(Serialize)]
struct PaymentView {
    id: String,
    amount_minor: u64,
    currency: String,
    status: PaymentStatus,
}

impl From<Payment> for PaymentView {
    fn from(payment: Payment) -> Self {
        Self {
            id: payment.id().to_string(),
            amount_minor: payment.amount().amount_minor(),
            currency: payment.amount().currency().as_str().to_owned(),
            status: payment.status().clone(),
        }
    }
}

async fn create<S: PaymentStore, P: PaymentProvider>(
    State(service): Service<S, P>,
    Path(tenant): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CreatePayment>,
) -> Result<Json<PaymentView>, ServiceError> {
    let tenant = TenantId::parse(&tenant)?;
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok());
    let key = IdempotencyKey::parse(key.unwrap_or_default())?;
    let amount = Money::new(body.amount_minor, Currency::parse(&body.currency)?)?;
    let payment = service.pay(tenant, key, amount).await?;
    Ok(Json(payment.into()))
}

async fn fetch<S: PaymentStore, P: PaymentProvider>(
    State(service): Service<S, P>,
    Path((tenant, id)): Path<(String, String)>,
) -> Result<Json<PaymentView>, ServiceError> {
    let tenant = TenantId::parse(&tenant)?;
    let payment = service.get(&tenant, &PaymentId::from_raw(id))?;
    Ok(Json(payment.into()))
}

async fn metrics<S: PaymentStore, P: PaymentProvider>(State(service): Service<S, P>) -> String {
    service.metrics().render()
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            Self::KeyReused => StatusCode::CONFLICT,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let message = if status.is_server_error() {
            tracing::error!(error = ?self, "request failed");
            "internal error".to_owned()
        } else {
            self.to_string()
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}
