// COBOL MCP Server - Mainframe Modernization Pipeline
// Compiles and executes COBOL programs using GnuCOBOL
// Endpoints:
//   POST /compile          - Compile and execute COBOL source
//   POST /execute          - Execute pre-compiled COBOL
//   POST /validate_syntax  - Validate COBOL syntax only
//   GET  /health           - Health check

use actix_web::{web, App, HttpServer, HttpResponse, middleware};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::fs;
use uuid::Uuid;
use log::{info, error};

// ─── Request/Response Types ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CompileRequest {
    pub source: String,           // COBOL source code
    pub input_data: Option<String>, // Optional stdin input
}

#[derive(Serialize)]
pub struct CompileResponse {
    pub success: bool,
    pub output: Option<String>,   // Execution output
    pub compile_log: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct ValidateRequest {
    pub source: String,
}

#[derive(Serialize)]
pub struct ValidateResponse {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// Compile COBOL source and execute it, returning stdout output
async fn compile(
    body: web::Json<CompileRequest>,
) -> HttpResponse {
    let job_id = Uuid::new_v4().to_string();
    let work_dir = format!("/tmp/cobol_{}", job_id);

    info!("Compiling COBOL job: {}", job_id);

    // Create working directory
    if let Err(e) = fs::create_dir_all(&work_dir) {
        return HttpResponse::InternalServerError().json(CompileResponse {
            success: false,
            output: None,
            compile_log: None,
            error: Some(format!("Failed to create work dir: {}", e)),
        });
    }

    let source_path = format!("{}/program.cbl", work_dir);
    let binary_path = format!("{}/program", work_dir);

    // Write COBOL source
    if let Err(e) = fs::write(&source_path, &body.source) {
        return HttpResponse::InternalServerError().json(CompileResponse {
            success: false,
            output: None,
            compile_log: None,
            error: Some(format!("Failed to write source: {}", e)),
        });
    }

    // Compile with GnuCOBOL
    let compile_result = Command::new("cobc")
        .args(["-x", "-o", &binary_path, &source_path])
        .output();

    match compile_result {
        Ok(output) => {
            let compile_log = String::from_utf8_lossy(&output.stderr).to_string();

            if !output.status.success() {
                error!("COBOL compile failed: {}", compile_log);
                cleanup(&work_dir);
                return HttpResponse::Ok().json(CompileResponse {
                    success: false,
                    output: None,
                    compile_log: Some(compile_log),
                    error: Some("COBOL compilation failed".to_string()),
                });
            }

            info!("COBOL compiled successfully, executing...");

            // Execute the compiled binary
            let mut exec_cmd = Command::new(&binary_path);

            // Provide input data if available
            if let Some(input) = &body.input_data {
                use std::process::Stdio;
                use std::io::Write;

                let child = exec_cmd
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|e| e.to_string());

                match child {
                    Ok(mut c) => {
                        if let Some(stdin) = c.stdin.take() {
                            let mut stdin = stdin;
                            let _ = stdin.write_all(input.as_bytes());
                        }
                        match c.wait_with_output() {
                            Ok(exec_output) => {
                                let stdout = String::from_utf8_lossy(&exec_output.stdout).to_string();
                                info!("COBOL execution output: {}", stdout.trim());
                                cleanup(&work_dir);
                                return HttpResponse::Ok().json(CompileResponse {
                                    success: true,
                                    output: Some(stdout),
                                    compile_log: Some(compile_log),
                                    error: None,
                                });
                            }
                            Err(e) => {
                                cleanup(&work_dir);
                                return HttpResponse::InternalServerError().json(CompileResponse {
                                    success: false,
                                    output: None,
                                    compile_log: Some(compile_log),
                                    error: Some(format!("Execution failed: {}", e)),
                                });
                            }
                        }
                    }
                    Err(e) => {
                        cleanup(&work_dir);
                        return HttpResponse::InternalServerError().json(CompileResponse {
                            success: false,
                            output: None,
                            compile_log: Some(compile_log),
                            error: Some(format!("Failed to spawn process: {}", e)),
                        });
                    }
                }
            }

            // No input data - execute directly
            match exec_cmd.output() {
                Ok(exec_output) => {
                    let stdout = String::from_utf8_lossy(&exec_output.stdout).to_string();
                    info!("COBOL output: {}", stdout.trim());
                    cleanup(&work_dir);
                    HttpResponse::Ok().json(CompileResponse {
                        success: true,
                        output: Some(stdout),
                        compile_log: Some(compile_log),
                        error: None,
                    })
                }
                Err(e) => {
                    cleanup(&work_dir);
                    HttpResponse::InternalServerError().json(CompileResponse {
                        success: false,
                        output: None,
                        compile_log: Some(compile_log),
                        error: Some(format!("Execution failed: {}", e)),
                    })
                }
            }
        }
        Err(e) => {
            error!("Failed to run cobc: {}", e);
            cleanup(&work_dir);
            HttpResponse::InternalServerError().json(CompileResponse {
                success: false,
                output: None,
                compile_log: None,
                error: Some(format!("cobc not found or failed: {}", e)),
            })
        }
    }
}

/// Validate COBOL syntax without executing
async fn validate_syntax(
    body: web::Json<ValidateRequest>,
) -> HttpResponse {
    let job_id = Uuid::new_v4().to_string();
    let work_dir = format!("/tmp/cobol_validate_{}", job_id);
    let _ = fs::create_dir_all(&work_dir);

    let source_path = format!("{}/program.cbl", work_dir);
    let _ = fs::write(&source_path, &body.source);

    // Use -fsyntax-only flag
    let result = Command::new("cobc")
        .args(["-fsyntax-only", &source_path])
        .output();

    cleanup(&work_dir);

    match result {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let valid = output.status.success();
            let errors: Vec<String> = if !valid {
                stderr.lines()
                    .filter(|l| l.contains("error"))
                    .map(String::from)
                    .collect()
            } else { vec![] };
            let warnings: Vec<String> = stderr.lines()
                .filter(|l| l.contains("warning"))
                .map(String::from)
                .collect();

            HttpResponse::Ok().json(ValidateResponse { valid, errors, warnings })
        }
        Err(e) => HttpResponse::InternalServerError().json(ValidateResponse {
            valid: false,
            errors: vec![e.to_string()],
            warnings: vec![],
        }),
    }
}

async fn health() -> HttpResponse {
    // Check if GnuCOBOL is available
    let cobc_available = Command::new("cobc").arg("--version").output().is_ok();
    HttpResponse::Ok().json(serde_json::json!({
        "status": "healthy",
        "service": "cobol-mcp",
        "version": "1.0.0",
        "gnucobol_available": cobc_available
    }))
}

fn cleanup(dir: &str) {
    let _ = fs::remove_dir_all(dir);
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or("0.0.0.0:8083".to_string());
    info!("⚙️  COBOL MCP Service starting on {}", bind_addr);

    HttpServer::new(|| {
        App::new()
            .wrap(middleware::Logger::default())
            .route("/compile", web::post().to(compile))
            .route("/execute", web::post().to(compile)) // same handler
            .route("/validate_syntax", web::post().to(validate_syntax))
            .route("/health", web::get().to(health))
    })
    .bind(&bind_addr)?
    .run()
    .await
}