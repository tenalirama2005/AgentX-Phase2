// AI MCP Server - Deprecated
// Replaced by purple_agent FBA consensus engine (arxiv:2507.11768)
// Kept for workspace compatibility only

use actix_web::{web, App, HttpResponse, HttpServer};

async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "healthy",
        "service": "ai_mcp",
        "version": "1.0.0",
        "note": "Deprecated — replaced by purple_agent FBA consensus"
    }))
}

async fn translate_cobol(_body: web::Json<serde_json::Value>) -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "success": false,
        "error": "ai_mcp is deprecated. Use purple_agent POST /modernize instead.",
        "purple_agent_url": "http://localhost:8081/modernize"
    }))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let bind_addr = std::env::var("BIND_ADDR")
        .unwrap_or("0.0.0.0:8084".to_string());

    log::info!("AI MCP (deprecated) starting on {}", bind_addr);
    log::info!("Use purple_agent at port 8081 instead");

    HttpServer::new(|| {
        App::new()
            .route("/health", web::get().to(health))
            .route("/translate_cobol", web::post().to(translate_cobol))
    })
    .bind(&bind_addr)?
    .run()
    .await
}