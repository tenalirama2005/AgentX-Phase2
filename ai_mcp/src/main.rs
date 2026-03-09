// AI Translation MCP Server - Mainframe Modernization Pipeline
// Uses Anthropic Claude claude-opus-4-6 to translate COBOL to idiomatic Rust
// Endpoints:
//   POST /translate_cobol      - Translate COBOL source to Rust
//   POST /translate_assembler  - Translate Assembler to Rust
//   POST /explain_code         - Explain COBOL code in plain English
//   GET  /health               - Health check

use actix_web::{web, App, HttpServer, HttpResponse, middleware};
use serde::{Deserialize, Serialize};
use log::{info, error};

const CLAUDE_API_URL: &str = "https://api.anthropic.com/v1/messages";
const CLAUDE_MODEL: &str = "claude-opus-4-6";

// ─── Request/Response Types ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TranslateRequest {
    pub source: String,
    pub context: Option<String>,
}

#[derive(Serialize)]
pub struct TranslateResponse {
    pub success: bool,
    pub rust_code: Option<String>,
    pub explanation: Option<String>,
    pub model_used: String,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct ExplainRequest {
    pub source: String,
}

#[derive(Serialize)]
pub struct ExplainResponse {
    pub success: bool,
    pub explanation: Option<String>,
    pub error: Option<String>,
}

// Claude API types
#[derive(Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<ClaudeMessage>,
}

#[derive(Serialize)]
struct ClaudeMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
}

#[derive(Deserialize)]
struct ClaudeContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

// ─── App State ────────────────────────────────────────────────────────────────

pub struct AppState {
    pub claude_api_key: String,
    pub http_client: reqwest::Client,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// Translate COBOL source to idiomatic Rust using Claude
async fn translate_cobol(
    state: web::Data<AppState>,
    body: web::Json<TranslateRequest>,
) -> HttpResponse {
    info!("Translating COBOL to Rust ({} chars)", body.source.len());

    let prompt = format!(
        r#"You are an expert COBOL and Rust programmer. Convert the following COBOL program to idiomatic, memory-safe Rust code.

Requirements:
1. The Rust code must produce IDENTICAL output to the COBOL program
2. Use idiomatic Rust with proper error handling
3. Always add `use rust_decimal::prelude::*;` at the top when using decimals
4. For decimal arithmetic use ONLY these approved methods:
   - Decimal::from_str() or dec!() macro to create decimals
   - Standard arithmetic operators: +, -, *, /
   - .round_dp(2) for rounding to 2 decimal places
   - .round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
   - .to_string() for converting to string
   - .to_f64().unwrap_or(0.0) for float conversion
   NEVER use: .inv(), .quantize(), RoundingStrategy::Truncate, RoundingStrategy::HalfUp
5. For formatting decimal output use: format!("{{:.2}}", value.to_f64().unwrap_or(0.0))
6. Match numeric formatting exactly (same decimal places, spacing)
7. Only use these crates: rust_decimal, rust_decimal_macros, num-format, num-traits, std
8. Return ONLY the complete Rust source code starting with use statements or fn main()
9. No explanations, no markdown code blocks, no backticks

COBOL Source:
{}

{}

Return ONLY the Rust source code."#,
        body.source,
        body.context.as_deref().unwrap_or("")
    );

    match call_claude(&state, &prompt).await {
        Ok(rust_code) => {
            // Clean up any markdown code blocks if present
            let clean_code = rust_code
                .trim()
                .trim_start_matches("```rust")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim()
                .to_string();

            info!("Translation complete: {} chars of Rust generated", clean_code.len());
            HttpResponse::Ok().json(TranslateResponse {
                success: true,
                rust_code: Some(clean_code),
                explanation: None,
                model_used: CLAUDE_MODEL.to_string(),
                error: None,
            })
        }
        Err(e) => {
            error!("Claude translation failed: {}", e);
            HttpResponse::InternalServerError().json(TranslateResponse {
                success: false,
                rust_code: None,
                explanation: None,
                model_used: CLAUDE_MODEL.to_string(),
                error: Some(e),
            })
        }
    }
}

/// Translate Assembler source to Rust
async fn translate_assembler(
    state: web::Data<AppState>,
    body: web::Json<TranslateRequest>,
) -> HttpResponse {
    info!("Translating Assembler to Rust ({} chars)", body.source.len());

    let prompt = format!(
        r#"You are an expert IBM mainframe Assembler (HLASM/BAL) and Rust programmer.
Convert the following Assembler program to idiomatic, memory-safe Rust code.

Requirements:
1. Produce IDENTICAL output to the Assembler program
2. Use idiomatic Rust with proper error handling
3. Always add `use rust_decimal::prelude::*;` at the top when using decimals
4. For decimal arithmetic use ONLY approved methods:
   - Standard arithmetic operators: +, -, *, /
   - .round_dp(2) for rounding
   NEVER use: .inv(), .quantize()
5. Only use crates: rust_decimal, rust_decimal_macros, std
6. Return ONLY the complete Rust source code
7. No explanations, no markdown code blocks, no backticks

Assembler Source:
{}

Return ONLY the Rust source code."#,
        body.source
    );

    match call_claude(&state, &prompt).await {
        Ok(rust_code) => {
            let clean_code = rust_code
                .trim()
                .trim_start_matches("```rust")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim()
                .to_string();

            HttpResponse::Ok().json(TranslateResponse {
                success: true,
                rust_code: Some(clean_code),
                explanation: None,
                model_used: CLAUDE_MODEL.to_string(),
                error: None,
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(TranslateResponse {
            success: false,
            rust_code: None,
            explanation: None,
            model_used: CLAUDE_MODEL.to_string(),
            error: Some(e),
        }),
    }
}

/// Explain COBOL code in plain English
async fn explain_code(
    state: web::Data<AppState>,
    body: web::Json<ExplainRequest>,
) -> HttpResponse {
    let prompt = format!(
        "Explain what this COBOL program does in plain English:\n\n{}",
        body.source
    );

    match call_claude(&state, &prompt).await {
        Ok(explanation) => HttpResponse::Ok().json(ExplainResponse {
            success: true,
            explanation: Some(explanation),
            error: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(ExplainResponse {
            success: false,
            explanation: None,
            error: Some(e),
        }),
    }
}

async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "healthy",
        "service": "ai-translation-mcp",
        "version": "1.0.0",
        "model": CLAUDE_MODEL
    }))
}

// ─── Claude API Helper ────────────────────────────────────────────────────────

async fn call_claude(state: &AppState, prompt: &str) -> Result<String, String> {
    let request = ClaudeRequest {
        model: CLAUDE_MODEL.to_string(),
        max_tokens: 32768,
        messages: vec![ClaudeMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }],
    };

    let response = state.http_client
        .post(CLAUDE_API_URL)
        .header("x-api-key", &state.claude_api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Claude API request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Claude API error {}: {}", status, text));
    }

    let claude_response: ClaudeResponse = response.json()
        .await
        .map_err(|e| format!("Failed to parse Claude response: {}", e))?;

    claude_response.content
        .into_iter()
        .find(|c| c.content_type == "text")
        .and_then(|c| c.text)
        .ok_or("Empty response from Claude".to_string())
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let claude_api_key = std::env::var("CLAUDE_API_KEY")
        .expect("CLAUDE_API_KEY must be set");

    let state = web::Data::new(AppState {
        claude_api_key,
        http_client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap(),
    });

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or("0.0.0.0:8082".to_string());
    info!("🤖 AI Translation MCP Service starting on {} using {}", bind_addr, CLAUDE_MODEL);

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .wrap(middleware::Logger::default())
            .route("/translate_cobol", web::post().to(translate_cobol))
            .route("/translate_assembler", web::post().to(translate_assembler))
            .route("/explain_code", web::post().to(explain_code))
            .route("/health", web::get().to(health))
    })
    .bind(&bind_addr)?
    .run()
    .await
}
