// purple_agent/src/main.rs — v0.4.0
// Wires run_consensus() into the /review handler (31-node FBA)
// Web framework: actix-web 4

use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

mod bayesian;
mod consensus;
mod fba;
mod nebius_client;

use consensus::{run_consensus, AppState, ConsensusConfig};

// ---------------------------------------------------------------------------
// Actix web-layer state wrapper
// ---------------------------------------------------------------------------

pub struct WebState {
    pub inner: AppState,
}

// ---------------------------------------------------------------------------
// Request / Response types — field names MUST match green_agent's
// PurpleAgentResponse and FbaNodeResult structs exactly
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ReviewRequest {
    pub cobol_source: String,
    pub review_id: Option<String>,
}

/// Matches green_agent's FbaNodeResult exactly
#[derive(Debug, Serialize)]
pub struct NodeSummary {
    pub node_id: String,
    pub model_name: String,
    pub rust_code: String,
    pub confidence: f64,
    pub cot_steps_used: usize,
}

/// Matches green_agent's PurpleAgentResponse
#[derive(Debug, Serialize)]
pub struct ReviewResponse {
    pub status: String,             // fba.status
    pub rust_code: Option<String>,  // fba.rust_code
    pub confidence: f64,            // fba.fba_confidence / fba.confidence
    pub bayesian_guarantee: String, // fba.bayesian_guarantee
    pub k_star: usize,              // fba.k_star
    pub semantic_similarity: f64,
    pub node_results: Vec<NodeSummary>, // fba.node_results
    pub paper_reference: String,
}

// ---------------------------------------------------------------------------
// /modernize + /review handler (green_agent calls /modernize)
// ---------------------------------------------------------------------------

async fn review_handler(
    state: web::Data<WebState>,
    req: web::Json<ReviewRequest>,
) -> impl Responder {
    let review_id = req
        .review_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    info!(review_id = %review_id, "Received request — starting 31-node consensus");

    let fba_result = run_consensus(&state.inner, &req.cobol_source).await;

    let node_results: Vec<NodeSummary> = fba_result
        .node_results
        .iter()
        .map(|n| NodeSummary {
            node_id: n.node_id.clone(),
            model_name: n.model_name.clone(),
            rust_code: n.rust_code.clone(),
            confidence: n.confidence,
            cot_steps_used: n.cot_steps_used,
        })
        .collect();

    info!(
        review_id = %review_id,
        status = %fba_result.status,
        bayesian_guarantee = %fba_result.bayesian_guarantee,
        confidence = fba_result.confidence,
        semantic_similarity = fba_result.semantic_similarity,
        nodes_responded = node_results.len(),
        "Consensus complete"
    );

    HttpResponse::Ok().json(ReviewResponse {
        status: fba_result.status,
        rust_code: fba_result.rust_code,
        confidence: fba_result.confidence,
        bayesian_guarantee: fba_result.bayesian_guarantee,
        k_star: fba_result.k_star,
        semantic_similarity: fba_result.semantic_similarity,
        node_results,
        paper_reference: fba_result.paper_reference,
    })
}

// ---------------------------------------------------------------------------
// Health check
// ---------------------------------------------------------------------------

async fn health_handler() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "service": "purple_agent",
        "version": "0.4.0"
    }))
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "purple_agent=info".into()),
        )
        .init();

    dotenvy::dotenv().ok();

    let config = ConsensusConfig::from_toml("models.toml").map_err(|e| {
        error!("Failed to load models.toml: {e}");
        std::io::Error::other(e.to_string())
    })?;

    info!(
        node_count = config.models.len(),
        "Loaded ConsensusConfig from models.toml"
    );

    let nebius_key = std::env::var("NEBIUS_API_KEY")
        .map_err(|_| std::io::Error::other("NEBIUS_API_KEY not set"))?;
    let anthropic_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| std::io::Error::other("ANTHROPIC_API_KEY not set"))?;

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3600))
        .build()
        .expect("Failed to build reqwest client");

    let state = web::Data::new(WebState {
        inner: AppState {
            http_client,
            nebius_key,
            anthropic_key,
            config,
        },
    });

    let addr = "0.0.0.0:8081";
    info!("purple_agent v0.4.0 listening on {addr}");

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .route("/modernize", web::post().to(review_handler)) // green_agent calls this
            .route("/review", web::post().to(review_handler)) // alias
            .route("/health", web::get().to(health_handler))
    })
    .keep_alive(std::time::Duration::from_secs(3600))
    .client_request_timeout(std::time::Duration::from_secs(3600))
    .bind(addr)?
    .run()
    .await
}
