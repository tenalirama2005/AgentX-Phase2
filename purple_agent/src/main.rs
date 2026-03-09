/// Purple Agent — FBA Consensus Engine for COBOL→Rust Modernization
/// AgentX Phase 2 | Sprint 1 | Mainframe-Modernization team
///
/// API:  POST http://localhost:8081/modernize
/// Team: Venkateshwar Rao Nagala (@venkatnagala)
/// Ref:  arxiv:2507.11768 — "LLMs are Bayesian, in Expectation, not in Realization"
mod bayesian;
mod claude;
mod consensus;
mod deepseek_v3;
mod fba;

use actix_web::{middleware, web, App, HttpResponse, HttpServer, Responder};
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client as S3Client;
use consensus::{run_consensus_pipeline, ConsensusConfig, ModernizeRequest};
use dotenvy::dotenv;
use serde::Serialize;
use std::env;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

// ── App state shared across requests ─────────────────────────────────────────

struct AppState {
    http_client: reqwest::Client,
    s3_client: S3Client,
    s3_bucket: String,
    anthropic_key: String,
    openai_key: String,
    consensus_config: ConsensusConfig,
}

// ── Health check ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    agent: String,
    version: String,
    paper_reference: String,
    endpoints: Vec<String>,
}

async fn health() -> impl Responder {
    HttpResponse::Ok().json(HealthResponse {
        status: "healthy".to_string(),
        agent: "purple_agent".to_string(),
        version: "0.2.0".to_string(),
        paper_reference: "arxiv:2507.11768".to_string(),
        endpoints: vec![
            "GET  /health".to_string(),
            "POST /modernize".to_string(),
            "GET  /config".to_string(),
        ],
    })
}

// ── Config endpoint ───────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ConfigResponse {
    model_primary: String,
    model_secondary: String,
    similarity_threshold: f64,
    confidence_threshold: f64,
    epsilon: f64,
    theta: f64,
    fba_network: String,
    s3_bucket: String,
}

async fn get_config(state: web::Data<AppState>) -> impl Responder {
    HttpResponse::Ok().json(ConfigResponse {
        model_primary: "claude-opus-4-6".to_string(),
        model_secondary: "deepseek_v3".to_string(),
        similarity_threshold: state.consensus_config.similarity_threshold,
        confidence_threshold: state.consensus_config.confidence_threshold,
        epsilon: state.consensus_config.epsilon,
        theta: state.consensus_config.theta,
        fba_network: "2-node (Claude + DeepSeek V3)".to_string(),
        s3_bucket: state.s3_bucket.clone(),
    })
}

// ── /modernize endpoint ───────────────────────────────────────────────────────

async fn modernize(
    state: web::Data<AppState>,
    body: web::Json<ModernizeRequest>,
) -> impl Responder {
    info!(
        "POST /modernize | cobol_lines={}",
        body.cobol_source.lines().count()
    );

    // Run the full FBA consensus pipeline
    let result = run_consensus_pipeline(
        &state.http_client,
        &state.anthropic_key,
        &state.openai_key,
        body.into_inner(),
        &state.consensus_config,
    )
    .await;

    match result {
        Ok(response) => {
            let status = response.status.clone();
            let action = response.action.clone();
            let request_id = response.request_id.clone();

            // If consensus reached, save to S3
            if status == "CONSENSUS_REACHED" {
                if let Some(ref rust_code) = response.rust_code {
                    let s3_key = format!("purple_agent/{}/output.rs", request_id);
                    match save_to_s3(&state.s3_client, &state.s3_bucket, &s3_key, rust_code).await {
                        Ok(_) => info!("✅ Saved to S3: s3://{}/{}", state.s3_bucket, s3_key),
                        Err(e) => warn!("S3 save failed (non-fatal): {}", e),
                    }
                }
            }

            info!(
                "Request {} complete: {} | action={}",
                request_id, status, action
            );

            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!("Consensus pipeline error: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": e.to_string(),
                "status": "PIPELINE_ERROR"
            }))
        }
    }
}

// ── S3 helper ─────────────────────────────────────────────────────────────────

async fn save_to_s3(
    client: &S3Client,
    bucket: &str,
    key: &str,
    content: &str,
) -> anyhow::Result<()> {
    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(content.as_bytes().to_vec().into())
        .content_type("text/x-rustsrc")
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("S3 PutObject failed: {}", e))?;

    Ok(())
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load .env file (optional — won't fail if missing)
    dotenv().ok();

    // Initialize structured logging
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive("purple_agent=info".parse().unwrap()))
        .init();

    info!("🟣 Purple Agent starting — FBA Consensus Engine");
    info!("   Paper: arxiv:2507.11768");
    info!("   Models: Claude Opus 4.6 (primary) + DeepSeek V3 (secondary)");

    // ── Load required environment variables ───────────────────────────────────
    let anthropic_key = env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");

    let openai_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");

    let s3_bucket =
        env::var("S3_BUCKET").unwrap_or_else(|_| "mainframe-refactor-lab-venkatnagala".to_string());

    let port: u16 = env::var("PORT")
        .unwrap_or_else(|_| "8081".to_string())
        .parse()
        .expect("PORT must be a number");

    // ── Optional consensus tuning via env vars ────────────────────────────────
    let consensus_config = ConsensusConfig {
        similarity_threshold: env::var("SIMILARITY_THRESHOLD")
            .unwrap_or_else(|_| "0.75".to_string())
            .parse()
            .unwrap_or(0.75),
        confidence_threshold: env::var("CONFIDENCE_THRESHOLD")
            .unwrap_or_else(|_| "0.85".to_string())
            .parse()
            .unwrap_or(0.85),
        epsilon: env::var("BAYESIAN_EPSILON")
            .unwrap_or_else(|_| "0.01".to_string())
            .parse()
            .unwrap_or(0.01),
        theta: env::var("BAYESIAN_THETA")
            .unwrap_or_else(|_| "2.5".to_string())
            .parse()
            .unwrap_or(2.5),
    };

    info!(
        "Config: similarity_threshold={} confidence_threshold={} ε={} Θ={}",
        consensus_config.similarity_threshold,
        consensus_config.confidence_threshold,
        consensus_config.epsilon,
        consensus_config.theta
    );

    // ── Build AWS S3 client ───────────────────────────────────────────────────
    let aws_config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let s3_client = S3Client::new(&aws_config);

    // ── Build shared HTTP client ──────────────────────────────────────────────
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("Failed to build HTTP client");

    let app_state = web::Data::new(AppState {
        http_client,
        s3_client,
        s3_bucket,
        anthropic_key,
        openai_key,
        consensus_config,
    });

    info!("🚀 Purple Agent listening on http://0.0.0.0:{}", port);

    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .app_data(web::JsonConfig::default().error_handler(|err, _req| {
                let response = HttpResponse::BadRequest().json(serde_json::json!({
                    "error": err.to_string()
                }));
                actix_web::error::InternalError::from_response(err, response).into()
            }))
            .wrap(middleware::Logger::default())
            .route("/health", web::get().to(health))
            .route("/config", web::get().to(get_config))
            .route("/modernize", web::post().to(modernize))
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
