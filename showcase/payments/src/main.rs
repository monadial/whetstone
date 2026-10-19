//! Runs the payments service: reads its configuration from the environment,
//! serves the HTTP API and shuts down gracefully on Ctrl+C.

use std::error::Error;
use std::sync::Arc;

use payments::cache::TenantCache;
use payments::config::Config;
use payments::provider::{HttpTransport, ProviderClient};
use payments::service::PaymentService;
use payments::store::FileStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::from_env()?;
    tracing_subscriber::fmt()
        .with_max_level(config.log_level)
        .init();
    std::fs::create_dir_all(&config.data_dir)?;
    let store = FileStore::open(&config.data_dir.join("payments.log"))?;
    let provider = ProviderClient::new(
        HttpTransport::new(&config.provider_url)?,
        config.provider_retry,
    );
    let cache = TenantCache::new(config.cache_per_tenant);
    let app = payments::http::router(Arc::new(PaymentService::new(store, provider, cache)));

    let listener = tokio::net::TcpListener::bind(config.listen_addr).await?;
    tracing::info!(address = %config.listen_addr, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            // If the signal handler cannot be installed, keep serving.
            if tokio::signal::ctrl_c().await.is_err() {
                std::future::pending::<()>().await;
            }
        })
        .await?;
    Ok(())
}
