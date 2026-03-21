// Rust MCP Server - Mainframe Modernization Pipeline
// Compiles and executes Rust programs using Cargo
// Endpoints:
//   POST /compile      - Compile and execute Rust source
//   POST /execute      - Execute pre-compiled Rust
//   POST /cargo_check  - Check Rust code without executing
//   POST /clippy       - Run Clippy lints
//   GET  /health       - Health check

use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::fs;
use std::process::Command;
use uuid::Uuid;

// ─── Request/Response Types ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CompileRequest {
    pub source: String,             // Rust source code
    pub input_data: Option<String>, // Optional stdin input
}

#[derive(Serialize)]
pub struct CompileResponse {
    pub success: bool,
    pub output: Option<String>, // Execution stdout
    pub compile_log: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct CheckRequest {
    pub source: String,
}

#[derive(Serialize)]
pub struct CheckResponse {
    pub success: bool,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// Compile Rust source and execute it, returning stdout output
async fn compile(body: web::Json<CompileRequest>) -> HttpResponse {
    let job_id = Uuid::new_v4().to_string();
    let work_dir = format!("/tmp/rust_{}", job_id);
    let src_dir = format!("{}/src", work_dir);

    info!("Compiling Rust job: {}", job_id);

    // Create Cargo project structure
    if let Err(e) = fs::create_dir_all(&src_dir) {
        return error_response(&format!("Failed to create work dir: {}", e));
    }

    // Detect dependencies from source code
    let mut deps = String::from("");
    if body.source.contains("rust_decimal") {
        deps.push_str("rust_decimal = \"1.34\"\n");
        deps.push_str("rust_decimal_macros = \"1.34\"\n");
    }
    if body.source.contains("num_format") {
        deps.push_str("num-format = { version = \"0.4\", features = [\"with-system-locale\"] }\n");
    }
    if body.source.contains("num_traits") || body.source.contains("num-traits") {
        deps.push_str("num-traits = \"0.2\"\n");
    }
    if body.source.contains("chrono") {
        deps.push_str("chrono = \"0.4\"\n");
    }
    if body.source.contains("regex") {
        deps.push_str("regex = \"1\"\n");
    }

    let cargo_toml = format!(
        r#"[package]
name = "modernized"
version = "0.1.0"
edition = "2021"

[dependencies]
{}
"#,
        deps
    );
    if let Err(e) = fs::write(format!("{}/Cargo.toml", work_dir), &cargo_toml) {
        cleanup(&work_dir);
        return error_response(&format!("Failed to write Cargo.toml: {}", e));
    }

    // Write main.rs
    if let Err(e) = fs::write(format!("{}/src/main.rs", work_dir), &body.source) {
        cleanup(&work_dir);
        return error_response(&format!("Failed to write source: {}", e));
    }

    // Build with cargo
    let build_output = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&work_dir)
        .env("CARGO_HOME", "/home/mcpuser/.cargo") // Shared cargo cache
        .output();

    match build_output {
        Ok(output) => {
            let compile_log = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );

            if !output.status.success() {
                error!("Rust compile failed:\n{}", compile_log);
                cleanup(&work_dir);
                return HttpResponse::Ok().json(CompileResponse {
                    success: false,
                    output: None,
                    compile_log: Some(compile_log),
                    error: Some("Rust compilation failed".to_string()),
                });
            }

            info!("Rust compiled successfully, executing...");

            // Execute the compiled binary
            let binary_path = format!("{}/target/release/modernized", work_dir);
            let exec_result = if let Some(input) = &body.input_data {
                use std::io::Write;
                use std::process::Stdio;

                let child = Command::new(&binary_path)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|e| e.to_string());

                match child {
                    Ok(mut c) => {
                        if let Some(mut stdin) = c.stdin.take() {
                            let _ = stdin.write_all(input.as_bytes());
                        }
                        c.wait_with_output()
                            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                            .map_err(|e| e.to_string())
                    }
                    Err(e) => Err(e),
                }
            } else {
                Command::new(&binary_path)
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                    .map_err(|e| e.to_string())
            };

            cleanup(&work_dir);

            match exec_result {
                Ok(stdout) => {
                    info!("Rust output: {}", stdout.trim());
                    HttpResponse::Ok().json(CompileResponse {
                        success: true,
                        output: Some(stdout),
                        compile_log: Some(compile_log),
                        error: None,
                    })
                }
                Err(e) => HttpResponse::InternalServerError().json(CompileResponse {
                    success: false,
                    output: None,
                    compile_log: Some(compile_log),
                    error: Some(format!("Execution failed: {}", e)),
                }),
            }
        }
        Err(e) => {
            cleanup(&work_dir);
            error_response(&format!("cargo not found: {}", e))
        }
    }
}

/// Check Rust code without executing (cargo check)
async fn cargo_check(body: web::Json<CheckRequest>) -> HttpResponse {
    let job_id = Uuid::new_v4().to_string();
    let work_dir = format!("/tmp/rust_check_{}", job_id);
    let src_dir = format!("{}/src", work_dir);
    let _ = fs::create_dir_all(&src_dir);

    let cargo_toml =
        "[package]\nname=\"check\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\n";
    let _ = fs::write(format!("{}/Cargo.toml", work_dir), cargo_toml);
    let _ = fs::write(format!("{}/src/main.rs", work_dir), &body.source);

    let result = Command::new("cargo")
        .args(["check"])
        .current_dir(&work_dir)
        .env("CARGO_HOME", "/home/mcpuser/.cargo")
        .output();

    cleanup(&work_dir);

    match result {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let success = output.status.success();
            let errors: Vec<String> = stderr
                .lines()
                .filter(|l| l.contains("error"))
                .map(String::from)
                .collect();
            let warnings: Vec<String> = stderr
                .lines()
                .filter(|l| l.contains("warning"))
                .map(String::from)
                .collect();
            HttpResponse::Ok().json(CheckResponse {
                success,
                warnings,
                errors,
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(CheckResponse {
            success: false,
            errors: vec![e.to_string()],
            warnings: vec![],
        }),
    }
}

/// Run Clippy lints
async fn clippy(body: web::Json<CheckRequest>) -> HttpResponse {
    let job_id = Uuid::new_v4().to_string();
    let work_dir = format!("/tmp/rust_clippy_{}", job_id);
    let src_dir = format!("{}/src", work_dir);
    let _ = fs::create_dir_all(&src_dir);

    let cargo_toml =
        "[package]\nname=\"clippy_check\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\n";
    let _ = fs::write(format!("{}/Cargo.toml", work_dir), cargo_toml);
    let _ = fs::write(format!("{}/src/main.rs", work_dir), &body.source);

    let result = Command::new("cargo")
        .args(["clippy", "--", "-D", "warnings"])
        .current_dir(&work_dir)
        .env("CARGO_HOME", "/home/mcpuser/.cargo")
        .output();

    cleanup(&work_dir);

    match result {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let success = output.status.success();
            HttpResponse::Ok().json(serde_json::json!({
                "success": success,
                "output": stderr
            }))
        }
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "error": e.to_string()
        })),
    }
}

async fn health() -> HttpResponse {
    let cargo_available = Command::new("cargo").arg("--version").output().is_ok();
    let rustc_version = Command::new("rustc")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or("unknown".to_string());

    HttpResponse::Ok().json(serde_json::json!({
        "status": "healthy",
        "service": "rust_mcp",
        "version": "1.0.0",
        "cargo_available": cargo_available,
        "rustc_version": rustc_version
    }))
}

fn cleanup(dir: &str) {
    let _ = fs::remove_dir_all(dir);
}

fn error_response(msg: &str) -> HttpResponse {
    HttpResponse::InternalServerError().json(CompileResponse {
        success: false,
        output: None,
        compile_log: None,
        error: Some(msg.to_string()),
    })
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or("0.0.0.0:8084".to_string());
    info!("🦀 Rust MCP Service starting on {}", bind_addr);

    HttpServer::new(|| {
        App::new()
            .wrap(middleware::Logger::default())
            .route("/compile", web::post().to(compile))
            .route("/execute", web::post().to(compile))
            .route("/cargo_check", web::post().to(cargo_check))
            .route("/clippy", web::post().to(clippy))
            .route("/health", web::get().to(health))
    })
    .bind(&bind_addr)?
    .run()
    .await
}
