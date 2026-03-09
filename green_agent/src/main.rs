// Green Agent - Orchestrator (Updated for Agent Gateway Integration)
// All MCP server calls now route through the Agent Gateway for AuthN/AuthZ
//
// Flow: Green Agent -> Agent Gateway (JWT) -> MCP Server
//       Previously: Green Agent -> MCP Server directly

use actix_web::{web, App, HttpResponse, HttpServer};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

// ─── Gateway Client ───────────────────────────────────────────────────────────

pub struct GatewayClient {
    pub gateway_url: String,
    pub agent_id: String,
    pub access_token: RwLock<Option<String>>,
    http_client: reqwest::Client,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
struct TokenResponse {
    access_token: String,
}

impl GatewayClient {
    pub fn new(gateway_url: String, agent_id: String, _api_key: String) -> Self {
        // Store api_key for token refresh (not shown for brevity - use Arc<Mutex>)
        GatewayClient {
            gateway_url,
            agent_id,
            access_token: RwLock::new(None),
            http_client: reqwest::Client::new(),
        }
    }

    /// Authenticate with gateway and get JWT token
    pub async fn authenticate(&self, api_key: &str) -> Result<(), String> {
        let response = self
            .http_client
            .post(format!("{}/auth/token", self.gateway_url))
            .json(&serde_json::json!({
                "agent_id": self.agent_id,
                "api_key": api_key,
                "requested_role": "orchestrator"
            }))
            .send()
            .await
            .map_err(|e| format!("Gateway auth failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("Auth rejected: {}", response.status()));
        }

        let token_resp: TokenResponse = response
            .json()
            .await
            .map_err(|e| format!("Invalid auth response: {}", e))?;

        let mut token = self.access_token.write().unwrap();
        *token = Some(token_resp.access_token);
        info!("✅ Green Agent authenticated with gateway");
        Ok(())
    }

    /// Call an MCP server via the gateway (requires prior authentication)
    pub async fn invoke_mcp(
        &self,
        target_mcp: &str,
        operation: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let token = {
            let t = self.access_token.read().unwrap();
            t.clone().ok_or("Not authenticated with gateway")?
        };

        let response = self
            .http_client
            .post(format!("{}/mcp/invoke", self.gateway_url))
            .header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "target_mcp": target_mcp,
                "operation": operation,
                "payload": payload
            }))
            .send()
            .await
            .map_err(|e| format!("Gateway request failed: {}", e))?;

        let status = response.status();

        // ── AUTO REFRESH on 401 ExpiredSignature ──────────────────────────
        if status.as_u16() == 401 {
            info!("🔄 JWT expired — refreshing token...");
            let api_key = std::env::var("AGENT_API_KEY").map_err(|_| "AGENT_API_KEY not set")?;
            self.authenticate(&api_key).await?;

            // Retry with new token
            let new_token = {
                let t = self.access_token.read().unwrap();
                t.clone().ok_or("Token missing after refresh")?
            };

            let retry = self
                .http_client
                .post(format!("{}/mcp/invoke", self.gateway_url))
                .header("Authorization", format!("Bearer {}", new_token))
                .json(&serde_json::json!({
                    "target_mcp": target_mcp,
                    "operation": operation,
                    "payload": payload
                }))
                .send()
                .await
                .map_err(|e| format!("Retry request failed: {}", e))?;

            let retry_status = retry.status();
            let retry_body: serde_json::Value = retry
                .json()
                .await
                .map_err(|e| format!("Invalid retry response: {}", e))?;

            return if retry_status.is_success() && retry_body["success"].as_bool().unwrap_or(false)
            {
                Ok(retry_body["result"].clone())
            } else {
                Err(format!(
                    "Gateway error after refresh {}: {:?}",
                    retry_status, retry_body
                ))
            };
        }
        // ─────────────────────────────────────────────────────────────────

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Invalid gateway response: {}", e))?;

        if status.is_success() {
            let success = body["success"].as_bool().unwrap_or(false);
            if success {
                Ok(body["result"].clone())
            } else {
                Err(body["error"]
                    .as_str()
                    .unwrap_or("Unknown error")
                    .to_string())
            }
        } else if status.as_u16() == 403 {
            Err(format!(
                "AuthZ DENIED for {}/{}: {}",
                target_mcp,
                operation,
                body["error"].as_str().unwrap_or("")
            ))
        } else {
            Err(format!("Gateway error {}: {:?}", status, body))
        }
    }
}

// ─── Pipeline Structs ─────────────────────────────────────────────────────────

//#[derive(Debug, Deserialize)]
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Hash)]
pub struct ModernizeRequest {
    pub task_id: String,
    pub source_location: SourceLocation,
}

//#[derive(Debug, Deserialize)]
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Hash)]
pub struct SourceLocation {
    pub bucket: String,
    pub key: String,
}

//#[derive(Debug, Serialize)]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct ModernizeResponse {
    pub task_id: String,
    pub status: String,
    pub match_confirmed: bool,
    pub rust_code_url: Option<String>,
    //pub logs_url: Option<String>,
    pub audit_request_id: Option<String>,
}

pub struct AppState {
    pub gateway: GatewayClient,
    pub s3_bucket: String,
}

// ─── Pipeline Handler ─────────────────────────────────────────────────────────

async fn evaluate(state: web::Data<AppState>, req: web::Json<ModernizeRequest>) -> HttpResponse {
    info!("🚀 Starting modernization for task: {}", req.task_id);
    let gw = &state.gateway;

    // Step 1: Fetch COBOL source via Agent Gateway -> S3 MCP
    let cobol_source = match gw
        .invoke_mcp(
            "s3_mcp",
            "fetch_source",
            serde_json::json!({
                "bucket": req.source_location.bucket,
                "key": req.source_location.key,
            }),
        )
        .await
    {
        Ok(result) => result["content"].as_str().unwrap_or("").to_string(),
        Err(e) => {
            error!("Failed to fetch COBOL: {}", e);
            return HttpResponse::InternalServerError().json(ModernizeResponse {
                task_id: req.task_id.clone(),
                status: format!("FAILED: {}", e),
                match_confirmed: false,
                rust_code_url: None,
                //logs_url: None,
                audit_request_id: None,
            });
        }
    };

    // Step 2: Compile and execute COBOL via Agent Gateway -> COBOL MCP
    let cobol_output = match gw
        .invoke_mcp(
            "cobol_mcp",
            "compile",
            serde_json::json!({"source": cobol_source}),
        )
        .await
    {
        Ok(r) => r["output"].as_str().unwrap_or("").to_string(),
        Err(e) => return error_response(&req.task_id, &e),
    };

    // Step 3: Translate to Rust via Agent Gateway -> AI MCP
    let rust_code = match gw
        .invoke_mcp(
            "ai_mcp",
            "translate_cobol",
            serde_json::json!({"source": cobol_source}),
        )
        .await
    {
        Ok(r) => r["rust_code"].as_str().unwrap_or("").to_string(),
        Err(e) => return error_response(&req.task_id, &e),
    };

    // Step 4: Compile and execute Rust via Agent Gateway -> Rust MCP
    let rust_output = match gw
        .invoke_mcp(
            "rust_mcp",
            "compile",
            serde_json::json!({"source": rust_code}),
        )
        .await
    {
        Ok(r) => r["output"].as_str().unwrap_or("").to_string(),
        Err(e) => return error_response(&req.task_id, &e),
    };

    // Step 5: Validate outputs match
    let match_confirmed = outputs_match(&cobol_output, &rust_output);
    info!(
        "Validation: COBOL={:?} RUST={:?} MATCH={}",
        cobol_output, rust_output, match_confirmed
    );

    // Step 6: Save to S3 only if validated
    let rust_code_url = if match_confirmed {
        let output_key = format!(
            "modernized/{}/{}.rs",
            req.task_id,
            req.source_location.key.replace(".cbl", "")
        );

        match gw
            .invoke_mcp(
                "s3_mcp",
                "save_output",
                serde_json::json!({
                    "bucket": state.s3_bucket,
                    "key": output_key,
                    "content": rust_code,
                }),
            )
            .await
        {
            Ok(r) => r["presigned_url"].as_str().map(String::from),
            Err(e) => {
                error!("Save failed: {}", e);
                None
            }
        }
    } else {
        None
    };

    HttpResponse::Ok().json(ModernizeResponse {
        task_id: req.task_id.clone(),
        status: if match_confirmed {
            "SUCCESS - Outputs match! ✅".to_string()
        } else {
            "MISMATCH - Needs review ⚠️".to_string()
        },
        match_confirmed,
        rust_code_url,
        //logs_url: None,
        audit_request_id: None, // Would be populated from gateway response
    })
}

fn outputs_match(cobol: &str, rust: &str) -> bool {
    // Normalize whitespace for comparison
    let normalize = |s: &str| -> String {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    normalize(cobol) == normalize(rust)
}

fn error_response(task_id: &str, error: &str) -> HttpResponse {
    HttpResponse::InternalServerError().json(ModernizeResponse {
        task_id: task_id.to_string(),
        status: format!("FAILED: {}", error),
        match_confirmed: false,
        rust_code_url: None,
        //logs_url: None,
        audit_request_id: None,
    })
}

async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({"status": "healthy", "agent": "green_agent"}))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let gateway_url =
        std::env::var("AGENT_GATEWAY_URL").unwrap_or("http://agent-gateway:8090".to_string());
    let agent_id = std::env::var("AGENT_ID").unwrap_or("green_agent".to_string());
    let api_key = std::env::var("AGENT_API_KEY").expect("AGENT_API_KEY must be set");
    let s3_bucket =
        std::env::var("S3_BUCKET").unwrap_or("mainframe-refactor-lab-venkatnagala".to_string());

    let gateway = GatewayClient::new(gateway_url.clone(), agent_id, api_key.clone());

    // Authenticate with gateway on startup
    gateway
        .authenticate(&api_key)
        .await
        .expect("Failed to authenticate with Agent Gateway");

    let state = web::Data::new(AppState { gateway, s3_bucket });

    info!(
        "🟢 Green Agent (Orchestrator) starting - Gateway: {}",
        gateway_url
    );

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .route("/evaluate", web::post().to(evaluate))
            .route("/health", web::get().to(health))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
