// Agent Gateway - Authentication & Authorization for MCP Servers
// Mainframe Modernization Pipeline - SOLO AI Competition 2026
//
// Architecture: Zero-trust gateway that validates all agent-to-MCP communication
// Uses JWT tokens + API key validation with role-based access control (RBAC)

use actix_web::web::Data;
use actix_web::{middleware, web, App, HttpRequest, HttpResponse, HttpServer};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use uuid::Uuid;

// ─── Data Structures ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct Claims {
    pub sub: String,               // Agent ID (green_agent, purple_agent)
    pub role: AgentRole,           // Role determines MCP server access
    pub exp: usize,                // Expiry timestamp
    pub iat: usize,                // Issued at
    pub jti: String,               // JWT ID for revocation
    pub allowed_mcps: Vec<String>, // Specific MCP servers this agent can call
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Orchestrator, // Green Agent: can call all MCPs
    Modernizer,   // Purple Agent: can only call AI translation MCPs
    ReadOnly,     // Audit/monitoring only
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct TokenRequest {
    pub agent_id: String,
    pub api_key: String, // Pre-shared API key for initial auth
    pub requested_role: AgentRole,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub agent_id: String,
    pub role: AgentRole,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct McpProxyRequest {
    pub target_mcp: String, // e.g., "s3_mcp", "ai_mcp", "cobol_mcp"
    pub operation: String,  // e.g., "fetch_source", "translate", "validate"
    pub payload: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct McpProxyResponse {
    pub success: bool,
    pub request_id: String,
    pub agent_id: String,
    pub target_mcp: String,
    pub operation: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub audit_trail: AuditEntry,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct AuditEntry {
    pub timestamp: String,
    pub agent_id: String,
    pub target_mcp: String,
    pub operation: String,
    pub authorized: bool,
    pub request_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub registered_mcps: Vec<String>,
    pub active_tokens: usize,
}

// ─── MCP Registry ─────────────────────────────────────────────────────────────

/// Maps MCP server names to their allowed operations per role
#[allow(clippy::new_without_default)]
impl Default for McpRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct McpRegistry {
    // mcp_name -> (url, allowed_roles_and_operations)
    servers: HashMap<String, McpServer>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpServer {
    pub url: String,
    pub allowed_operations: HashMap<AgentRole, Vec<String>>,
}

impl McpRegistry {
    pub fn new() -> Self {
        let mut servers = HashMap::new();

        // S3 MCP: Handles all AWS S3 operations
        let mut s3_ops: HashMap<AgentRole, Vec<String>> = HashMap::new();
        s3_ops.insert(
            AgentRole::Orchestrator,
            vec![
                "fetch_source".to_string(),
                "fetch_data".to_string(),
                "save_output".to_string(),
                "generate_presigned_url".to_string(),
                "list_objects".to_string(),
            ],
        );
        s3_ops.insert(AgentRole::ReadOnly, vec!["list_objects".to_string()]);
        servers.insert(
            "s3_mcp".to_string(),
            McpServer {
                url: std::env::var("S3_MCP_URL").unwrap_or("http://s3_mcp:8081".to_string()),
                allowed_operations: s3_ops,
            },
        );

        // AI MCP: AI translation operations
        let mut ai_ops: HashMap<AgentRole, Vec<String>> = HashMap::new();
        ai_ops.insert(
            AgentRole::Orchestrator,
            vec![
                "translate_cobol".to_string(),
                "translate_assembler".to_string(),
                "explain_code".to_string(),
            ],
        );
        ai_ops.insert(
            AgentRole::Modernizer,
            vec![
                "translate_cobol".to_string(),
                "translate_assembler".to_string(),
            ],
        );
        servers.insert(
            "ai_mcp".to_string(),
            McpServer {
                url: std::env::var("AI_MCP_URL").unwrap_or("http://ai_mcp:8082".to_string()),
                allowed_operations: ai_ops,
            },
        );

        // COBOL Compiler MCP: GnuCOBOL compilation & execution
        let mut cobol_ops: HashMap<AgentRole, Vec<String>> = HashMap::new();
        cobol_ops.insert(
            AgentRole::Orchestrator,
            vec![
                "compile".to_string(),
                "execute".to_string(),
                "validate_syntax".to_string(),
            ],
        );
        servers.insert(
            "cobol_mcp".to_string(),
            McpServer {
                url: std::env::var("COBOL_MCP_URL").unwrap_or("http://cobol_mcp:8083".to_string()),
                allowed_operations: cobol_ops,
            },
        );

        // Rust Compiler MCP: Cargo compilation & execution
        let mut rust_ops: HashMap<AgentRole, Vec<String>> = HashMap::new();
        rust_ops.insert(
            AgentRole::Orchestrator,
            vec![
                "compile".to_string(),
                "execute".to_string(),
                "cargo_check".to_string(),
                "clippy".to_string(),
            ],
        );
        servers.insert(
            "rust_mcp".to_string(),
            McpServer {
                url: std::env::var("RUST_MCP_URL").unwrap_or("http://rust_mcp:8084".to_string()),
                allowed_operations: rust_ops,
            },
        );

        McpRegistry { servers }
    }

    pub fn is_authorized(&self, role: &AgentRole, mcp: &str, operation: &str) -> bool {
        if let Some(server) = self.servers.get(mcp) {
            if let Some(allowed_ops) = server.allowed_operations.get(role) {
                return allowed_ops.contains(&operation.to_string());
            }
        }
        false
    }

    pub fn get_server_url(&self, mcp: &str) -> Option<String> {
        self.servers.get(mcp).map(|s| s.url.clone())
    }

    pub fn list_servers(&self) -> Vec<String> {
        self.servers.keys().cloned().collect()
    }
}

// ─── App State ────────────────────────────────────────────────────────────────
#[allow(clippy::new_without_default)]
impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AppState {
    pub jwt_secret: String,
    pub api_keys: RwLock<HashMap<String, (String, AgentRole)>>, // api_key -> (agent_id, role)
    pub revoked_tokens: RwLock<Vec<String>>,                    // revoked JWT IDs
    pub audit_log: RwLock<Vec<AuditEntry>>,
    pub mcp_registry: McpRegistry,
}

impl AppState {
    pub fn new() -> Self {
        let mut api_keys = HashMap::new();

        // Pre-configured agent API keys (in production: use Kubernetes secrets)
        let green_key = std::env::var("GREEN_AGENT_API_KEY")
            .unwrap_or("green-agent-dev-key-change-in-prod".to_string());
        let purple_key = std::env::var("PURPLE_AGENT_API_KEY")
            .unwrap_or("purple-agent-dev-key-change-in-prod".to_string());

        api_keys.insert(
            green_key,
            ("green_agent".to_string(), AgentRole::Orchestrator),
        );
        api_keys.insert(
            purple_key,
            ("purple_agent".to_string(), AgentRole::Modernizer),
        );

        AppState {
            jwt_secret: std::env::var("JWT_SECRET")
                .unwrap_or("dev-secret-change-in-production-minimum-32-chars".to_string()),
            api_keys: RwLock::new(api_keys),
            revoked_tokens: RwLock::new(Vec::new()),
            audit_log: RwLock::new(Vec::new()),
            mcp_registry: McpRegistry::new(),
        }
    }
}

// ─── Token Issuance ───────────────────────────────────────────────────────────

async fn issue_token(state: Data<AppState>, req: web::Json<TokenRequest>) -> HttpResponse {
    let api_keys = state.api_keys.read().unwrap();

    // Validate API key and agent identity
    let (agent_id, allowed_role) = match api_keys.get(&req.api_key) {
        Some((id, role)) => (id.clone(), role.clone()),
        None => {
            warn!("Invalid API key attempt for agent: {}", req.agent_id);
            return HttpResponse::Unauthorized().json(serde_json::json!({
                "error": "Invalid API key",
                "code": "AUTH_FAILED"
            }));
        }
    };

    // Verify agent_id matches API key
    if agent_id != req.agent_id {
        warn!(
            "Agent ID mismatch: claimed={}, actual={}",
            req.agent_id, agent_id
        );
        return HttpResponse::Forbidden().json(serde_json::json!({
            "error": "Agent ID does not match API key",
            "code": "IDENTITY_MISMATCH"
        }));
    }

    // Cannot escalate privileges beyond assigned role
    if req.requested_role != allowed_role {
        warn!(
            "Role escalation attempt by {}: requested {:?}, allowed {:?}",
            agent_id, req.requested_role, allowed_role
        );
        return HttpResponse::Forbidden().json(serde_json::json!({
            "error": "Requested role exceeds allowed permissions",
            "code": "PRIVILEGE_ESCALATION"
        }));
    }

    // Determine allowed MCP servers for this role
    let allowed_mcps = state
        .mcp_registry
        .list_servers()
        .into_iter()
        .filter(|mcp| {
            // Check if this role has ANY access to this MCP
            state.mcp_registry.mcp_registry_has_role(mcp, &allowed_role)
        })
        .collect::<Vec<_>>();

    let now = Utc::now();
    let expiry = now + Duration::hours(1);
    let jti = Uuid::new_v4().to_string();

    let claims = Claims {
        sub: agent_id.clone(),
        role: allowed_role.clone(),
        exp: expiry.timestamp() as usize,
        iat: now.timestamp() as usize,
        jti: jti.clone(),
        allowed_mcps: allowed_mcps.clone(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )
    .unwrap();

    info!(
        "Token issued for agent: {} with role: {:?}",
        agent_id, allowed_role
    );

    HttpResponse::Ok().json(TokenResponse {
        access_token: token,
        token_type: "Bearer".to_string(),
        expires_in: 3600,
        agent_id,
        role: allowed_role,
    })
}

// ─── MCP Proxy with AuthZ ─────────────────────────────────────────────────────

async fn proxy_mcp_request(
    state: Data<AppState>,
    http_req: HttpRequest,
    body: web::Json<McpProxyRequest>,
) -> HttpResponse {
    let request_id = Uuid::new_v4().to_string();

    // Extract and validate Bearer token
    let token = match extract_bearer_token(&http_req) {
        Some(t) => t,
        None => {
            return HttpResponse::Unauthorized().json(serde_json::json!({
                "error": "Missing Authorization header",
                "code": "NO_TOKEN"
            }));
        }
    };

    let claims = match validate_token(&token, &state) {
        Ok(c) => c,
        Err(e) => {
            warn!("Token validation failed: {}", e);
            return HttpResponse::Unauthorized().json(serde_json::json!({
                "error": e,
                "code": "INVALID_TOKEN"
            }));
        }
    };

    // Authorization check: can this agent call this MCP with this operation?
    let authorized =
        state
            .mcp_registry
            .is_authorized(&claims.role, &body.target_mcp, &body.operation);

    // Record in audit log regardless of outcome
    let audit_entry = AuditEntry {
        timestamp: Utc::now().to_rfc3339(),
        agent_id: claims.sub.clone(),
        target_mcp: body.target_mcp.clone(),
        operation: body.operation.clone(),
        authorized,
        request_id: request_id.clone(),
    };

    {
        let mut log = state.audit_log.write().unwrap();
        log.push(audit_entry.clone());
        // Keep only last 10000 entries in memory
        if log.len() > 10000 {
            log.drain(0..1000);
        }
    }

    if !authorized {
        warn!(
            "AuthZ DENIED: agent={} role={:?} mcp={} op={}",
            claims.sub, claims.role, body.target_mcp, body.operation
        );
        return HttpResponse::Forbidden().json(McpProxyResponse {
            success: false,
            request_id,
            agent_id: claims.sub,
            target_mcp: body.target_mcp.clone(),
            operation: body.operation.clone(),
            result: None,
            error: Some(format!(
                "Role {:?} is not authorized to call {} on {}",
                claims.role, body.operation, body.target_mcp
            )),
            audit_trail: audit_entry,
        });
    }

    // Forward to MCP server
    let mcp_url = match state.mcp_registry.get_server_url(&body.target_mcp) {
        Some(url) => url,
        None => {
            return HttpResponse::NotFound().json(McpProxyResponse {
                success: false,
                request_id,
                agent_id: claims.sub,
                target_mcp: body.target_mcp.clone(),
                operation: body.operation.clone(),
                result: None,
                error: Some(format!("MCP server '{}' not registered", body.target_mcp)),
                audit_trail: audit_entry,
            });
        }
    };

    info!(
        "AuthZ OK: agent={} -> mcp={} op={} req_id={}",
        claims.sub, body.target_mcp, body.operation, request_id
    );

    // In production this proxies to actual MCP; for demo returns success
    let mcp_result = call_mcp_server(&mcp_url, &body.operation, &body.payload, &request_id).await;

    match mcp_result {
        Ok(result) => HttpResponse::Ok().json(McpProxyResponse {
            success: true,
            request_id,
            agent_id: claims.sub,
            target_mcp: body.target_mcp.clone(),
            operation: body.operation.clone(),
            result: Some(result),
            error: None,
            audit_trail: audit_entry,
        }),
        Err(e) => {
            error!("MCP call failed: {}", e);
            HttpResponse::InternalServerError().json(McpProxyResponse {
                success: false,
                request_id,
                agent_id: claims.sub,
                target_mcp: body.target_mcp.clone(),
                operation: body.operation.clone(),
                result: None,
                error: Some(e),
                audit_trail: audit_entry,
            })
        }
    }
}

async fn call_mcp_server(
    url: &str,
    operation: &str,
    payload: &serde_json::Value,
    request_id: &str,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .post(format!("{}/{}", url, operation))
        .header("X-Request-ID", request_id)
        .header("X-Gateway", "agent_gateway/1.0")
        .json(payload)
        .send()
        .await
        .map_err(|e| format!("MCP unreachable: {}", e))?;

    if response.status().is_success() {
        response
            .json::<serde_json::Value>()
            .await
            .map_err(|e| format!("Invalid MCP response: {}", e))
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        Err(format!("MCP returned {}: {}", status, text))
    }
}

// ─── Audit Log Endpoint ───────────────────────────────────────────────────────

async fn get_audit_log(state: Data<AppState>, http_req: HttpRequest) -> HttpResponse {
    let token = match extract_bearer_token(&http_req) {
        Some(t) => t,
        None => return HttpResponse::Unauthorized().finish(),
    };

    let claims = match validate_token(&token, &state) {
        Ok(c) => c,
        Err(_) => return HttpResponse::Unauthorized().finish(),
    };

    // Only orchestrators can view audit logs
    if claims.role != AgentRole::Orchestrator {
        return HttpResponse::Forbidden().json(serde_json::json!({
            "error": "Audit log access requires Orchestrator role"
        }));
    }

    let log = state.audit_log.read().unwrap();
    HttpResponse::Ok().json(serde_json::json!({
        "total_entries": log.len(),
        "entries": *log
    }))
}

async fn health_check(state: Data<AppState>) -> HttpResponse {
    let revoked_count = state.revoked_tokens.read().unwrap().len();
    HttpResponse::Ok().json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        registered_mcps: state.mcp_registry.list_servers(),
        active_tokens: revoked_count, // tokens issued minus revoked
    })
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn extract_bearer_token(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.to_string())
}

fn validate_token(token: &str, state: &AppState) -> Result<Claims, String> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map_err(|e| format!("Token decode failed: {}", e))?;

    let claims = token_data.claims;

    // Check revocation list
    let revoked = state.revoked_tokens.read().unwrap();
    if revoked.contains(&claims.jti) {
        return Err("Token has been revoked".to_string());
    }

    Ok(claims)
}

// Extend McpRegistry with helper method
impl McpRegistry {
    fn mcp_registry_has_role(&self, mcp: &str, role: &AgentRole) -> bool {
        if let Some(server) = self.servers.get(mcp) {
            return server.allowed_operations.contains_key(role);
        }
        false
    }
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let state = Data::new(AppState::new());
    let bind_addr = std::env::var("BIND_ADDR").unwrap_or("0.0.0.0:8090".to_string());

    info!("🔐 Agent Gateway starting on {}", bind_addr);
    info!(
        "📋 Registered MCP servers: {:?}",
        state.mcp_registry.list_servers()
    );

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .wrap(middleware::Logger::default())
            // Auth endpoints
            .route("/auth/token", web::post().to(issue_token))
            // MCP proxy endpoint (requires Bearer token)
            .route("/mcp/invoke", web::post().to(proxy_mcp_request))
            // Audit and monitoring
            .route("/audit/log", web::get().to(get_audit_log))
            .route("/health", web::get().to(health_check))
    })
    .bind(&bind_addr)?
    .run()
    .await
}
