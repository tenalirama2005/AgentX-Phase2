/// DeepSeek V3 API Client
/// Mirrors the Claude client interface exactly for FBA symmetry.
/// Calls OpenAI's /v1/chat/completions endpoint.
use crate::bayesian::build_cot_suffix;
use crate::fba::FbaNode;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

const OPENAI_API_URL: &str = "https://api.deepseek.com/v1/chat/completions";
const MODEL: &str = "deepseek-chat"; // DeepSeek V3

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<OpenAiMessage>,
    temperature: f64,
}

#[derive(Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Deserialize, Debug)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize, Debug)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct OpenAiResponseMessage {
    content: Option<String>,
}

#[derive(Deserialize, Debug)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

// ── Prompts ───────────────────────────────────────────────────────────────────

fn build_system_prompt() -> String {
    r#"You are an expert COBOL-to-Rust modernization engineer.
Your task is to translate COBOL source code into idiomatic, production-quality Rust.

Rules:
1. Preserve ALL business logic exactly — no shortcuts
2. Match numeric precision (use f64 for decimal calculations)
3. Use descriptive variable names matching COBOL data names where possible
4. Include a brief doc comment explaining what the function does
5. The Rust code must compile with `cargo build` without errors
6. After your reasoning steps, output EXACTLY this format:

RUST_CODE:
```rust
[your complete Rust implementation here]
```

CONFIDENCE: [a number between 0.0 and 1.0]

The confidence score reflects:
- 1.0 = you are certain the Rust is semantically identical to the COBOL
- 0.0 = you are guessing
- Be honest — undersell rather than oversell"#
        .to_string()
}

fn build_user_prompt(cobol_source: &str, k_star: usize) -> String {
    let cot_suffix = build_cot_suffix(k_star);
    format!(
        "Translate the following COBOL program to Rust:\n\n\
        ```cobol\n{cobol_source}\n```\
        {cot_suffix}"
    )
}

// ── Response parser ───────────────────────────────────────────────────────────

fn parse_deepseek_v3_response(raw: &str) -> (String, f64) {
    let rust_code = extract_rust_code(raw);
    let confidence = extract_confidence(raw);
    (rust_code, confidence)
}

fn extract_rust_code(text: &str) -> String {
    if let Some(start) = text.find("```rust") {
        let after_fence = &text[start + 7..];
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim().to_string();
        }
    }

    if let Some(start) = text.find("RUST_CODE:") {
        let after_marker = &text[start + 10..];
        let code = if let Some(end) = after_marker.find("CONFIDENCE:") {
            &after_marker[..end]
        } else {
            after_marker
        };
        return code.trim().to_string();
    }

    text.trim().to_string()
}

fn extract_confidence(text: &str) -> f64 {
    if let Some(pos) = text.find("CONFIDENCE:") {
        let after = &text[pos + 11..];
        let token = after.split_whitespace().next().unwrap_or("0.5");
        let clean = token.trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.');
        return clean.parse::<f64>().unwrap_or(0.5).clamp(0.0, 1.0);
    }
    0.5
}

// ── Main client function ──────────────────────────────────────────────────────

/// Call DeepSeek V3 to translate COBOL → Rust
/// Returns an FbaNode ready for consensus evaluation
pub async fn call_deepseek_v3(
    client: &reqwest::Client,
    api_key: &str,
    cobol_source: &str,
    k_star: usize,
) -> Result<FbaNode> {
    info!("Calling DeepSeek V3 with k*={} CoT steps", k_star);

    let request_body = OpenAiRequest {
        model: MODEL.to_string(),
        max_tokens: 4096,
        temperature: 0.2, // Low temperature for deterministic code generation
        messages: vec![
            OpenAiMessage {
                role: "system".to_string(),
                content: build_system_prompt(),
            },
            OpenAiMessage {
                role: "user".to_string(),
                content: build_user_prompt(cobol_source, k_star),
            },
        ],
    };

    let response = client
        .post(OPENAI_API_URL)
        .bearer_auth(api_key)
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| anyhow!("DeepSeek V3 API request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        error!("DeepSeek V3 API error {}: {}", status, body);
        return Err(anyhow!("DeepSeek V3 API returned {}: {}", status, body));
    }

    let parsed: OpenAiResponse = response
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse DeepSeek V3 response: {}", e))?;

    if let Some(usage) = &parsed.usage {
        info!(
            "DeepSeek V3 token usage: {} in / {} out",
            usage.prompt_tokens, usage.completion_tokens
        );
    }

    let finish_reason = parsed
        .choices
        .first()
        .and_then(|c| c.finish_reason.as_deref())
        .unwrap_or("unknown");
    info!("DeepSeek V3 finish_reason: {}", finish_reason);

    let raw_text = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .unwrap_or_default();

    let (rust_code, confidence) = parse_deepseek_v3_response(&raw_text);

    info!(
        "DeepSeek V3 response parsed: confidence={:.3}, code_len={}",
        confidence,
        rust_code.len()
    );

    Ok(FbaNode {
        node_id: "deepseek_v3".to_string(),
        model_name: "DeepSeek V3".to_string(),
        rust_code,
        confidence,
        cot_steps_used: k_star,
        raw_response: raw_text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rust_code_deepseek_v3_format() {
        let response = r#"
STEP 1: Analyze COBOL structure
STEP 2: Map data types

RUST_CODE:
```rust
/// Calculate simple interest
fn calculate_interest(principal: f64, rate: f64) -> f64 {
    (principal * rate) / 100.0
}
```

CONFIDENCE: 0.92
        "#;

        let (code, confidence) = parse_deepseek_v3_response(response);
        assert!(code.contains("calculate_interest"));
        assert!((confidence - 0.92).abs() < 0.001);
    }
}
