// Green Agent - Orchestrator v2.0
// Wires: s3_mcp → cobol_mcp → purple_agent → s3_mcp
//
// Pipeline:
//   Step 1: Fetch COBOL from S3           via s3_mcp      (port 8082)
//   Step 2: Compile & validate COBOL      via cobol_mcp   (port 8083)
//   Step 3: FBA consensus (Claude+DeepSeek) via purple_agent (port 8081)
//   Step 4: Save verified Rust to S3      via s3_mcp      (port 8082)
//
// Endpoints:
//   POST /modernize       - Run full pipeline for a COBOL file
//   POST /modernize_batch - Run pipeline for multiple COBOL files
//   GET  /health          - Health check (pings all 3 services)
//
// arxiv:2507.11768 — Bayesian-in-Realization FBA guarantee

use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use serde::{Deserialize, Serialize};
use log::{error, info, warn};

// ─── Port Constants ───────────────────────────────────────────────────────────
const DEFAULT_S3_MCP_URL: &str = "http://localhost:8082";
const DEFAULT_COBOL_MCP_URL: &str = "http://localhost:8083";
const DEFAULT_PURPLE_AGENT_URL: &str = "http://localhost:8081";
const DEFAULT_PORT: &str = "0.0.0.0:8080";
const S3_BUCKET: &str = "mainframe-refactor-lab-venkatnagala";
const COBOL_PREFIX: &str = "programs/";
const RUST_PREFIX: &str = "modernized/";

// ─── App State ────────────────────────────────────────────────────────────────

pub struct AppState {
    pub http_client: reqwest::Client,
    pub s3_mcp_url: String,
    pub cobol_mcp_url: String,
    pub purple_agent_url: String,
    pub s3_bucket: String,
}

// ─── Request/Response Types ───────────────────────────────────────────────────

/// Request to modernize a single COBOL file from S3
#[derive(Deserialize)]
pub struct ModernizeRequest {
    /// S3 key e.g. "programs/interest_calc.cbl"
    pub s3_key: String,
    /// Skip FBA consensus (default: false)
    #[serde(default)]
    pub skip_fba: bool,
}

/// Request to modernize multiple COBOL files
#[derive(Deserialize)]
pub struct BatchModernizeRequest {
    pub s3_keys: Vec<String>,
    #[serde(default)]
    pub skip_fba: bool,
}

/// Full pipeline response
#[derive(Serialize, Deserialize)]
pub struct ModernizeResponse {
    pub status: String,
    pub s3_input_key: String,
    pub s3_output_key: Option<String>,
    pub presigned_url: Option<String>,
    pub cobol_output: Option<String>,
    pub fba_status: Option<String>,
    pub fba_confidence: Option<f64>,
    pub bayesian_guarantee: Option<String>,
    pub k_star: Option<u32>,
    pub semantic_similarity: Option<f64>,
    pub paper_reference: String,
    pub error: Option<String>,
}

/// Batch response
#[derive(Serialize, Deserialize)]
pub struct BatchModernizeResponse {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub results: Vec<ModernizeResponse>,
}

// ─── S3 MCP Types ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct S3FetchRequest {
    bucket: String,
    key: String,
}

#[derive(Deserialize)]
struct S3FetchResponse {
    success: bool,
    content: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct S3SaveRequest {
    bucket: String,
    key: String,
    content: String,
}

#[derive(Deserialize)]
struct S3SaveResponse {
    success: bool,
    key: String,
    presigned_url: Option<String>,
    error: Option<String>,
}

// ─── COBOL MCP Types ──────────────────────────────────────────────────────────

#[derive(Serialize)]
struct CobolCompileRequest {
    source: String,
    input_data: Option<String>,
}

#[derive(Deserialize)]
struct CobolCompileResponse {
    success: bool,
    output: Option<String>,
    compile_log: Option<String>,
    error: Option<String>,
}

// ─── Purple Agent Types ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct PurpleAgentRequest {
    cobol_source: String,
}

#[derive(Deserialize)]
struct PurpleAgentResponse {
    status: String,
    rust_code: Option<String>,
    confidence: f64,
    bayesian_guarantee: String,
    k_star: u32,
    semantic_similarity: f64,
    paper_reference: String,
}

// ─── Pipeline Steps ───────────────────────────────────────────────────────────

/// Step 1: Fetch COBOL source from S3 via s3_mcp
async fn step1_fetch_cobol(
    client: &reqwest::Client,
    s3_mcp_url: &str,
    bucket: &str,
    key: &str,
) -> Result<String, String> {
    info!("📥 Step 1: Fetching COBOL from s3://{}/{}", bucket, key);

    let response = client
        .post(format!("{}/fetch_source", s3_mcp_url))
        .json(&S3FetchRequest {
            bucket: bucket.to_string(),
            key: key.to_string(),
        })
        .send()
        .await
        .map_err(|e| format!("s3_mcp unreachable: {}", e))?;

    let result: S3FetchResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid s3_mcp response: {}", e))?;

    if result.success {
        let content = result.content.ok_or("S3 returned empty content")?;
        info!("✅ Step 1: Fetched {} bytes of COBOL", content.len());
        Ok(content)
    } else {
        Err(result.error.unwrap_or("S3 fetch failed".to_string()))
    }
}

/// Step 2: Compile & validate COBOL via cobol_mcp
async fn step2_compile_cobol(
    client: &reqwest::Client,
    cobol_mcp_url: &str,
    cobol_source: &str,
) -> Result<String, String> {
    info!("⚙️  Step 2: Compiling COBOL via cobol_mcp");

    let response = client
        .post(format!("{}/compile", cobol_mcp_url))
        .json(&CobolCompileRequest {
            source: cobol_source.to_string(),
            input_data: None,
        })
        .send()
        .await
        .map_err(|e| format!("cobol_mcp unreachable: {}", e))?;

    let result: CobolCompileResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid cobol_mcp response: {}", e))?;

    if result.success {
        let output = result.output.unwrap_or_default();
        info!("✅ Step 2: COBOL compiled — output: {}", output.trim());
        Ok(output)
    } else {
        let err = result.error.unwrap_or_default();
        let log = result.compile_log.unwrap_or_default();
        Err(format!("COBOL compile failed: {} | log: {}", err, log))
    }
}

/// Step 3: FBA consensus via purple_agent (Claude Opus 4.6 + DeepSeek V3)
async fn step3_fba_consensus(
    client: &reqwest::Client,
    purple_agent_url: &str,
    cobol_source: &str,
) -> Result<PurpleAgentResponse, String> {
    info!("🟣 Step 3: FBA consensus via purple_agent (arxiv:2507.11768)");

    let response = client
        .post(format!("{}/modernize", purple_agent_url))
        .json(&PurpleAgentRequest {
            cobol_source: cobol_source.to_string(),
        })
        .timeout(std::time::Duration::from_secs(180))
        .send()
        .await
        .map_err(|e| format!("purple_agent unreachable: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("purple_agent HTTP {}: {}", status, body));
    }

    let result: PurpleAgentResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid purple_agent response: {}", e))?;

    info!(
        "✅ Step 3: FBA {} | confidence={:.3} | similarity={:.3} | bayesian={} | k*={}",
        result.status, result.confidence, result.semantic_similarity,
        result.bayesian_guarantee, result.k_star
    );

    Ok(result)
}

/// Step 4: Save verified Rust code to S3 via s3_mcp
async fn step4_save_rust(
    client: &reqwest::Client,
    s3_mcp_url: &str,
    bucket: &str,
    output_key: &str,
    rust_code: &str,
) -> Result<(String, Option<String>), String> {
    info!("📤 Step 4: Saving Rust to s3://{}/{}", bucket, output_key);

    let response = client
        .post(format!("{}/save_output", s3_mcp_url))
        .json(&S3SaveRequest {
            bucket: bucket.to_string(),
            key: output_key.to_string(),
            content: rust_code.to_string(),
        })
        .send()
        .await
        .map_err(|e| format!("s3_mcp unreachable on save: {}", e))?;

    let result: S3SaveResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid s3_mcp save response: {}", e))?;

    if result.success {
        info!("✅ Step 4: Rust saved to s3://{}/{}", bucket, result.key);
        Ok((result.key, result.presigned_url))
    } else {
        Err(result.error.unwrap_or("S3 save failed".to_string()))
    }
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// POST /modernize — Full pipeline for one COBOL file
async fn modernize(
    state: web::Data<AppState>,
    req: web::Json<ModernizeRequest>,
) -> HttpResponse {
    info!("🟢 Green Agent: Starting pipeline for {}", req.s3_key);

    let paper_ref = "arxiv:2507.11768".to_string();

    // Derive output key: programs/interest_calc.cbl → modernized/interest_calc.rs
    let filename = req.s3_key
        .trim_start_matches(COBOL_PREFIX)
        .replace(".cbl", ".rs");
    let output_key = format!("{}{}", RUST_PREFIX, filename);

    // ── Step 1: Fetch COBOL ──────────────────────────────────────────────────
    let cobol_source = match step1_fetch_cobol(
        &state.http_client, &state.s3_mcp_url,
        &state.s3_bucket, &req.s3_key,
    ).await {
        Ok(s) => s,
        Err(e) => {
            error!("❌ Step 1 failed: {}", e);
            return HttpResponse::InternalServerError().json(ModernizeResponse {
                status: format!("FAILED at Step 1 (S3 fetch): {}", e),
                s3_input_key: req.s3_key.clone(),
                s3_output_key: None, presigned_url: None,
                cobol_output: None, fba_status: None,
                fba_confidence: None, bayesian_guarantee: None,
                k_star: None, semantic_similarity: None,
                paper_reference: paper_ref, error: Some(e),
            });
        }
    };

    // ── Step 2: Compile COBOL ────────────────────────────────────────────────
    let cobol_output = match step2_compile_cobol(
        &state.http_client, &state.cobol_mcp_url, &cobol_source,
    ).await {
        Ok(o) => o,
        Err(e) => {
            error!("❌ Step 2 failed: {}", e);
            return HttpResponse::InternalServerError().json(ModernizeResponse {
                status: format!("FAILED at Step 2 (COBOL compile): {}", e),
                s3_input_key: req.s3_key.clone(),
                s3_output_key: None, presigned_url: None,
                cobol_output: None, fba_status: None,
                fba_confidence: None, bayesian_guarantee: None,
                k_star: None, semantic_similarity: None,
                paper_reference: paper_ref, error: Some(e),
            });
        }
    };

    // ── Step 3: FBA Consensus ────────────────────────────────────────────────
    let (rust_code, fba_status, fba_confidence, bayesian_guarantee, k_star, similarity) =
        if req.skip_fba {
            warn!("⏭️  FBA skipped (skip_fba=true)");
            (String::new(), Some("SKIPPED".to_string()), None, None, None, None)
        } else {
            match step3_fba_consensus(
                &state.http_client, &state.purple_agent_url, &cobol_source,
            ).await {
                Ok(fba) => {
                    if fba.status == "CONSENSUS_REACHED" {
                        let code = fba.rust_code.unwrap_or_default();
                        (
                            code,
                            Some(fba.status),
                            Some(fba.confidence),
                            Some(fba.bayesian_guarantee),
                            Some(fba.k_star),
                            Some(fba.semantic_similarity),
                        )
                    } else {
                        warn!("⚠️ FBA {} — needs human review", fba.status);
                        return HttpResponse::Ok().json(ModernizeResponse {
                            status: format!("FBA {} — needs human review ⚠️", fba.status),
                            s3_input_key: req.s3_key.clone(),
                            s3_output_key: None, presigned_url: None,
                            cobol_output: Some(cobol_output),
                            fba_status: Some(fba.status),
                            fba_confidence: Some(fba.confidence),
                            bayesian_guarantee: Some(fba.bayesian_guarantee),
                            k_star: Some(fba.k_star),
                            semantic_similarity: Some(fba.semantic_similarity),
                            paper_reference: fba.paper_reference,
                            error: None,
                        });
                    }
                }
                Err(e) => {
                    warn!("⚠️ Purple Agent unavailable: {}", e);
                    (String::new(), Some("UNAVAILABLE".to_string()), None, None, None, None)
                }
            }
        };

    if rust_code.is_empty() {
        return HttpResponse::Ok().json(ModernizeResponse {
            status: "FAILED — no Rust code produced".to_string(),
            s3_input_key: req.s3_key.clone(),
            s3_output_key: None, presigned_url: None,
            cobol_output: Some(cobol_output),
            fba_status, fba_confidence, bayesian_guarantee,
            k_star, semantic_similarity: similarity,
            paper_reference: paper_ref,
            error: Some("No Rust code from FBA".to_string()),
        });
    }

    // ── Step 4: Save Rust to S3 ──────────────────────────────────────────────
    let (saved_key, presigned_url) = match step4_save_rust(
        &state.http_client, &state.s3_mcp_url,
        &state.s3_bucket, &output_key, &rust_code,
    ).await {
        Ok((k, u)) => (Some(k), u),
        Err(e) => { error!("❌ Step 4 failed: {}", e); (None, None) }
    };

    let status_msg = match fba_status.as_deref() {
        Some("CONSENSUS_REACHED") =>
            "✅ SUCCESS — FBA Consensus (Bayesian-in-Realization)".to_string(),
        Some("SKIPPED") => "✅ SUCCESS — FBA skipped".to_string(),
        Some("UNAVAILABLE") => "⚠️ PARTIAL — Purple Agent offline".to_string(),
        Some(other) => format!("⚠️ PARTIAL — FBA: {}", other),
        None => "✅ SUCCESS".to_string(),
    };

    info!("🏁 Pipeline complete: {} → {}", req.s3_key, output_key);

    HttpResponse::Ok().json(ModernizeResponse {
        status: status_msg,
        s3_input_key: req.s3_key.clone(),
        s3_output_key: saved_key,
        presigned_url,
        cobol_output: Some(cobol_output),
        fba_status,
        fba_confidence,
        bayesian_guarantee,
        k_star,
        semantic_similarity: similarity,
        paper_reference: paper_ref,
        error: None,
    })
}

/// POST /modernize_batch — Run pipeline for multiple COBOL files
async fn modernize_batch(
    state: web::Data<AppState>,
    req: web::Json<BatchModernizeRequest>,
) -> HttpResponse {
    info!("🟢 Batch pipeline for {} files", req.s3_keys.len());
    let total = req.s3_keys.len();
    let mut results = Vec::new();
    let mut succeeded = 0usize;
    let mut failed = 0usize;

    for key in &req.s3_keys {
        let single = ModernizeRequest { s3_key: key.clone(), skip_fba: req.skip_fba };
        let resp = modernize(state.clone(), web::Json(single)).await;
        let bytes = actix_web::body::to_bytes(resp.into_body()).await.unwrap_or_default();
        if let Ok(r) = serde_json::from_slice::<ModernizeResponse>(&bytes) {
            if r.status.contains("SUCCESS") { succeeded += 1; } else { failed += 1; }
            results.push(r);
        }
    }

    HttpResponse::Ok().json(BatchModernizeResponse { total, succeeded, failed, results })
}

/// GET /health — Ping all 3 downstream services
async fn health(state: web::Data<AppState>) -> HttpResponse {
    let s3_ok = state.http_client
        .get(format!("{}/health", state.s3_mcp_url))
        .send().await.map(|r| r.status().is_success()).unwrap_or(false);

    let cobol_ok = state.http_client
        .get(format!("{}/health", state.cobol_mcp_url))
        .send().await.map(|r| r.status().is_success()).unwrap_or(false);

    let purple_ok = state.http_client
        .get(format!("{}/health", state.purple_agent_url))
        .send().await.map(|r| r.status().is_success()).unwrap_or(false);

    let all_ok = s3_ok && cobol_ok && purple_ok;

    let body = serde_json::json!({
        "status": if all_ok { "healthy" } else { "degraded" },
        "agent": "green_agent",
        "version": "2.0.0",
        "pipeline": {
            "s3_mcp":       { "url": state.s3_mcp_url,       "healthy": s3_ok },
            "cobol_mcp":    { "url": state.cobol_mcp_url,    "healthy": cobol_ok },
            "purple_agent": { "url": state.purple_agent_url, "healthy": purple_ok }
        },
        "s3_bucket": state.s3_bucket,
        "fba_paper": "arxiv:2507.11768"
    });

    if all_ok { HttpResponse::Ok().json(body) }
    else { HttpResponse::ServiceUnavailable().json(body) }
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let s3_mcp_url      = std::env::var("S3_MCP_URL").unwrap_or(DEFAULT_S3_MCP_URL.to_string());
    let cobol_mcp_url   = std::env::var("COBOL_MCP_URL").unwrap_or(DEFAULT_COBOL_MCP_URL.to_string());
    let purple_agent_url = std::env::var("PURPLE_AGENT_URL").unwrap_or(DEFAULT_PURPLE_AGENT_URL.to_string());
    let s3_bucket       = std::env::var("S3_BUCKET").unwrap_or(S3_BUCKET.to_string());
    let bind_addr       = std::env::var("BIND_ADDR").unwrap_or(DEFAULT_PORT.to_string());

    info!("🟢 Green Agent v2.0 starting on {}", bind_addr);
    info!("   s3_mcp:       {}", s3_mcp_url);
    info!("   cobol_mcp:    {}", cobol_mcp_url);
    info!("   purple_agent: {}", purple_agent_url);
    info!("   S3 bucket:    {}", s3_bucket);
    info!("   FBA paper:    arxiv:2507.11768");

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .expect("Failed to build HTTP client");

    let state = web::Data::new(AppState {
        http_client, s3_mcp_url, cobol_mcp_url, purple_agent_url, s3_bucket,
    });

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .wrap(middleware::Logger::default())
            .route("/modernize", web::post().to(modernize))
            .route("/modernize_batch", web::post().to(modernize_batch))
            .route("/health", web::get().to(health))
    })
    .bind(&bind_addr)?
    .run()
    .await
}