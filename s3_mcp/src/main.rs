// S3 MCP Server - Mainframe Modernization Pipeline
// Handles all AWS S3 operations for the pipeline
// Endpoints:
//   POST /fetch_source       - Fetch COBOL source file from S3
//   POST /fetch_data         - Fetch test data from S3
//   POST /save_output        - Save modernized Rust code to S3
//   POST /generate_presigned_url - Generate pre-signed URL for download
//   POST /list_objects       - List objects in bucket/prefix

use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use aws_config::BehaviorVersion;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::Client;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::time::Duration;

// ─── Request/Response Types ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct FetchRequest {
    pub bucket: String,
    pub key: String,
}

#[derive(Deserialize)]
pub struct SaveRequest {
    pub bucket: String,
    pub key: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct ListRequest {
    pub bucket: String,
    pub prefix: Option<String>,
}

#[derive(Serialize)]
pub struct FetchResponse {
    pub success: bool,
    pub bucket: String,
    pub key: String,
    pub content: Option<String>,
    pub size: Option<usize>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct SaveResponse {
    pub success: bool,
    pub bucket: String,
    pub key: String,
    pub presigned_url: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct ListResponse {
    pub success: bool,
    pub bucket: String,
    pub objects: Vec<String>,
    pub error: Option<String>,
}

// ─── App State ────────────────────────────────────────────────────────────────

pub struct AppState {
    pub s3_client: Client,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// Fetch COBOL source file from S3
async fn fetch_source(state: web::Data<AppState>, body: web::Json<FetchRequest>) -> HttpResponse {
    info!("Fetching source: s3://{}/{}", body.bucket, body.key);

    match get_s3_object(&state.s3_client, &body.bucket, &body.key).await {
        Ok(content) => {
            let size = content.len();
            info!(
                "Fetched {} bytes from s3://{}/{}",
                size, body.bucket, body.key
            );
            HttpResponse::Ok().json(FetchResponse {
                success: true,
                bucket: body.bucket.clone(),
                key: body.key.clone(),
                content: Some(content),
                size: Some(size),
                error: None,
            })
        }
        Err(e) => {
            error!("Failed to fetch s3://{}/{}: {}", body.bucket, body.key, e);
            HttpResponse::InternalServerError().json(FetchResponse {
                success: false,
                bucket: body.bucket.clone(),
                key: body.key.clone(),
                content: None,
                size: None,
                error: Some(e),
            })
        }
    }
}

/// Fetch test data from S3
async fn fetch_data(state: web::Data<AppState>, body: web::Json<FetchRequest>) -> HttpResponse {
    info!("Fetching data: s3://{}/{}", body.bucket, body.key);

    match get_s3_object(&state.s3_client, &body.bucket, &body.key).await {
        Ok(content) => HttpResponse::Ok().json(FetchResponse {
            success: true,
            bucket: body.bucket.clone(),
            key: body.key.clone(),
            content: Some(content.clone()),
            size: Some(content.len()),
            error: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(FetchResponse {
            success: false,
            bucket: body.bucket.clone(),
            key: body.key.clone(),
            content: None,
            size: None,
            error: Some(e),
        }),
    }
}

/// Save modernized Rust code to S3 and return pre-signed URL
async fn save_output(state: web::Data<AppState>, body: web::Json<SaveRequest>) -> HttpResponse {
    info!("Saving output: s3://{}/{}", body.bucket, body.key);

    // Upload to S3
    let put_result = state
        .s3_client
        .put_object()
        .bucket(&body.bucket)
        .key(&body.key)
        .body(body.content.as_bytes().to_vec().into())
        .content_type("text/plain")
        .send()
        .await;

    match put_result {
        Ok(_) => {
            info!(
                "Saved {} bytes to s3://{}/{}",
                body.content.len(),
                body.bucket,
                body.key
            );

            // Generate pre-signed URL for download (1 hour expiry)
            let presigned_url = generate_presigned(&state.s3_client, &body.bucket, &body.key).await;

            HttpResponse::Ok().json(SaveResponse {
                success: true,
                bucket: body.bucket.clone(),
                key: body.key.clone(),
                presigned_url,
                error: None,
            })
        }
        Err(e) => {
            error!("Failed to save to s3://{}/{}: {}", body.bucket, body.key, e);
            HttpResponse::InternalServerError().json(SaveResponse {
                success: false,
                bucket: body.bucket.clone(),
                key: body.key.clone(),
                presigned_url: None,
                error: Some(e.to_string()),
            })
        }
    }
}

/// Generate pre-signed URL for an existing S3 object
async fn generate_presigned_url(
    state: web::Data<AppState>,
    body: web::Json<FetchRequest>,
) -> HttpResponse {
    let url = generate_presigned(&state.s3_client, &body.bucket, &body.key).await;

    match url {
        Some(u) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "presigned_url": u,
            "expires_in": 3600
        })),
        None => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "error": "Failed to generate pre-signed URL"
        })),
    }
}

/// List objects in S3 bucket/prefix
async fn list_objects(state: web::Data<AppState>, body: web::Json<ListRequest>) -> HttpResponse {
    let mut req = state.s3_client.list_objects_v2().bucket(&body.bucket);

    if let Some(prefix) = &body.prefix {
        req = req.prefix(prefix);
    }

    match req.send().await {
        Ok(output) => {
            let objects: Vec<String> = output
                .contents()
                .iter()
                .filter_map(|obj| obj.key().map(String::from))
                .collect();

            HttpResponse::Ok().json(ListResponse {
                success: true,
                bucket: body.bucket.clone(),
                objects,
                error: None,
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(ListResponse {
            success: false,
            bucket: body.bucket.clone(),
            objects: vec![],
            error: Some(e.to_string()),
        }),
    }
}

async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "healthy",
        "service": "s3-mcp",
        "version": "1.0.0"
    }))
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

async fn get_s3_object(client: &Client, bucket: &str, key: &str) -> Result<String, String> {
    let response = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| format!("S3 GetObject failed: {}", e))?;

    let bytes = response
        .body
        .collect()
        .await
        .map_err(|e| format!("Failed to read S3 body: {}", e))?;

    String::from_utf8(bytes.into_bytes().to_vec())
        .map_err(|e| format!("Invalid UTF-8 in S3 object: {}", e))
}

async fn generate_presigned(client: &Client, bucket: &str, key: &str) -> Option<String> {
    let presigning_config = PresigningConfig::expires_in(Duration::from_secs(3600)).ok()?;

    client
        .get_object()
        .bucket(bucket)
        .key(key)
        .presigned(presigning_config)
        .await
        .ok()
        .map(|p| p.uri().to_string())
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // Load AWS config from environment
    let aws_config = aws_config::defaults(BehaviorVersion::latest()).load().await;

    let s3_client = Client::new(&aws_config);
    let state = web::Data::new(AppState { s3_client });

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or("0.0.0.0:8081".to_string());
    info!("🪣 S3 MCP Service starting on {}", bind_addr);

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .wrap(middleware::Logger::default())
            .route("/fetch_source", web::post().to(fetch_source))
            .route("/fetch_data", web::post().to(fetch_data))
            .route("/save_output", web::post().to(save_output))
            .route(
                "/generate_presigned_url",
                web::post().to(generate_presigned_url),
            )
            .route("/list_objects", web::post().to(list_objects))
            .route("/health", web::get().to(health))
    })
    .bind(&bind_addr)?
    .run()
    .await
}
