use round_robin_quorum::{Network, api};
use tokio::net::TcpListener;
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
    
    tracing::info!(%address, "RoundRobinQuorum server started");
    
    // Create network directly - api::app() handles Arc<RwLock> wrapping internally
    let network = Network::new();
    tracing::info!("Server is ready to accept requests");
    
    axum::serve(listener, api::app(network))
        .await?;
    
    Ok(())
}

