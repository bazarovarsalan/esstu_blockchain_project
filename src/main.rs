use round_robin_quorum::{Network, api};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "round_robin_quorum=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let address = std::env::var("RRQ_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
    let listener = TcpListener::bind(&address).await?;
    
    // Initialize network in background while accepting connections
    let network = Arc::new(RwLock::new(Network::new()));
    
    tracing::info!(%address, "RoundRobinQuorum server started");
    tracing::info!("Server is ready to accept requests");
    
    axum::serve(listener, api::app((*network.read().await).clone()))
        .await?;
    
    Ok(())
}

