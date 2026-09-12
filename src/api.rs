use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use tokio::sync::RwLock;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::{
    model::{
        Block, ConsensusAttempt, CreateTransactionRequest, IntegrityOverview, NetworkSnapshot,
        ScenarioReport, Transaction, ValidatorStatusRequest,
    },
    network::{Network, NetworkError},
};

pub type SharedNetwork = Arc<RwLock<Network>>;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

#[derive(Serialize)]
struct ApiErrorBody {
    code: &'static str,
    message: String,
}

impl From<NetworkError> for ApiError {
    fn from(error: NetworkError) -> Self {
        let (status, code) = match &error {
            NetworkError::UnknownAccount(_)
            | NetworkError::UnknownValidator(_)
            | NetworkError::BlockNotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            NetworkError::NoActiveNode => (StatusCode::SERVICE_UNAVAILABLE, "no_active_node"),
            NetworkError::WalletUnavailable(_) => (StatusCode::FORBIDDEN, "wallet_unavailable"),
            NetworkError::Validation(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation"),
            NetworkError::CryptoFormat | NetworkError::Demo(_) => {
                (StatusCode::BAD_REQUEST, "bad_request")
            }
        };
        Self {
            status,
            code,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiErrorBody {
                code: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}

pub fn app(network: Network) -> Router {
    let state = Arc::new(RwLock::new(network));
    Router::new()
        .route("/", get(root))
        .route("/api/health", get(health))
        .route("/api/state", get(get_state))
        .route("/api/integrity", get(get_integrity))
        .route("/api/blocks/{height}", get(get_block))
        .route("/api/transactions", post(create_transaction))
        .route("/api/transactions/submit", post(submit_transaction))
        .route("/api/consensus/produce", post(produce_block))
        .route("/api/validators/{id}/status", post(set_validator_status))
        .route("/api/demo/reset", post(reset))
        .route("/api/demo/scenarios/{name}", post(run_scenario))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn root() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "RoundRobinQuorum API",
        "scope": "educational single-process simulation",
        "endpoints": {
            "health": "GET /api/health",
            "state": "GET /api/state",
            "integrity": "GET /api/integrity",
            "block": "GET /api/blocks/{height}",
            "create_transaction": "POST /api/transactions",
            "submit_transaction": "POST /api/transactions/submit",
            "produce_block": "POST /api/consensus/produce",
            "set_validator": "POST /api/validators/{id}/status",
            "reset": "POST /api/demo/reset",
            "scenario": "POST /api/demo/scenarios/{name}"
        }
    }))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "protocol": "RoundRobinQuorum",
        "scope": "educational single-process simulation"
    }))
}

async fn get_state(State(network): State<SharedNetwork>) -> Json<NetworkSnapshot> {
    Json(network.read().await.snapshot())
}

async fn get_integrity(State(network): State<SharedNetwork>) -> Json<IntegrityOverview> {
    Json(network.read().await.integrity())
}

async fn get_block(
    State(network): State<SharedNetwork>,
    Path(height): Path<u64>,
) -> Result<Json<Block>, ApiError> {
    Ok(Json(network.read().await.block_at(height)?))
}

async fn create_transaction(
    State(network): State<SharedNetwork>,
    Json(request): Json<CreateTransactionRequest>,
) -> Result<(StatusCode, Json<Transaction>), ApiError> {
    let transaction = network.write().await.create_and_submit_transaction(
        &request.sender,
        &request.recipient,
        request.amount,
        request.nonce,
    )?;
    Ok((StatusCode::CREATED, Json(transaction)))
}

async fn submit_transaction(
    State(network): State<SharedNetwork>,
    Json(transaction): Json<Transaction>,
) -> Result<StatusCode, ApiError> {
    network.write().await.submit_transaction(transaction)?;
    Ok(StatusCode::ACCEPTED)
}

async fn produce_block(
    State(network): State<SharedNetwork>,
) -> Result<Json<ConsensusAttempt>, ApiError> {
    Ok(Json(network.write().await.produce_block()?))
}

async fn set_validator_status(
    State(network): State<SharedNetwork>,
    Path(id): Path<String>,
    Json(request): Json<ValidatorStatusRequest>,
) -> Result<Json<NetworkSnapshot>, ApiError> {
    let mut network = network.write().await;
    network.set_validator_active(&id, request.active)?;
    Ok(Json(network.snapshot()))
}

async fn reset(State(network): State<SharedNetwork>) -> Json<NetworkSnapshot> {
    let mut network = network.write().await;
    network.reset();
    Json(network.snapshot())
}

async fn run_scenario(
    State(network): State<SharedNetwork>,
    Path(name): Path<String>,
) -> Result<Json<ScenarioReport>, ApiError> {
    Ok(Json(network.write().await.run_scenario(&name)?))
}

