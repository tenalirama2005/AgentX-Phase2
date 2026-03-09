/// Claude Opus 4.6 API Client
/// Calls Anthropic's /v1/messages endpoint to translate COBOL → Rust
/// with FBA-optimized chain-of-thought depth (k* steps)
use crate::bayesian::build_cot_suffix;
use crate::fba::FbaNode;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";
const MODEL: &str = "claude-opus-4-6";

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<AnthropicMessage>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize, Debug)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    usage: Option<AnthropicUsage>,
}

#[derive(Deserialize, Debug)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

// ── System prompt ─────────────────────────────────────────────────────────────

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

// ── User prompt builder ───────────────────────────────────────────────────────

fn build_user_prompt(cobol_source: &str, k_star: usize) -> String {
    let cot_suffix = build_cot_suffix(k_star);
    format!(
        "Translate the following COBOL program to Rust:\n\n\
        ```cobol\n{cobol_source}\n```\
        {cot_suffix}"
    )
}

// ── Response parser ───────────────────────────────────────────────────────────

/// Extract Rust code and confidence from Claude's response text
fn parse_claude_response(raw: &str) -> (String, f64) {
    // Extract Rust code block
    let rust_code = extract_rust_code(raw);

    // Extract confidence score
    let confidence = extract_confidence(raw);

    (rust_code, confidence)
}

fn extract_rust_code(text: &str) -> String {
    // Try to find ```rust ... ``` block after RUST_CODE:
    if let Some(start) = text.find("```rust") {
        let after_fence = &text[start + 7..];
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim().to_string();
        }
    }

    // Fallback: find RUST_CODE: marker and take everything until CONFIDENCE:
    if let Some(start) = text.find("RUST_CODE:") {
        let after_marker = &text[start + 10..];
        let code = if let Some(end) = after_marker.find("CONFIDENCE:") {
            &after_marker[..end]
        } else {
            after_marker
        };
        return code.trim().to_string();
    }

    // Last resort: return entire response
    text.trim().to_string()
}

fn extract_confidence(text: &str) -> f64 {
    // Find "CONFIDENCE: X.XX" pattern
    if let Some(pos) = text.find("CONFIDENCE:") {
        let after = &text[pos + 11..];
        // Take first token after "CONFIDENCE:"
        let token = after.split_whitespace().next().unwrap_or("0.5");
        let clean = token.trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.');
        return clean.parse::<f64>().unwrap_or(0.5).clamp(0.0, 1.0);
    }
    0.5 // Default if not found
}

// ── Main client function ──────────────────────────────────────────────────────

/// Call Claude Opus 4.6 to translate COBOL → Rust
/// Returns an FbaNode ready for consensus evaluation
pub async fn call_claude(
    client: &reqwest::Client,
    api_key: &str,
    cobol_source: &str,
    k_star: usize,
) -> Result<FbaNode> {
    info!("Calling Claude Opus 4.6 with k*={} CoT steps", k_star);

    let request_body = AnthropicRequest {
        model: MODEL.to_string(),
        max_tokens: 4096,
        system: build_system_prompt(),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: build_user_prompt(cobol_source, k_star),
        }],
    };

    let response = client
        .post(ANTHROPIC_API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_API_VERSION)
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| anyhow!("Claude API request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        error!("Claude API error {}: {}", status, body);
        return Err(anyhow!("Claude API returned {}: {}", status, body));
    }

    let parsed: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse Claude response: {}", e))?;

    if let Some(usage) = &parsed.usage {
        info!(
            "Claude token usage: {} in / {} out",
            usage.input_tokens, usage.output_tokens
        );
    }

    // Extract text from content blocks
    let raw_text = parsed
        .content
        .iter()
        .filter(|c| c.content_type == "text")
        .filter_map(|c| c.text.as_deref())
        .collect::<Vec<_>>()
        .join("\n");

    let (rust_code, confidence) = parse_claude_response(&raw_text);

    info!(
        "Claude response parsed: confidence={:.3}, code_len={}",
        confidence,
        rust_code.len()
    );

    Ok(FbaNode {
        node_id: "claude_opus_4_6".to_string(),
        model_name: "Claude Opus 4.6".to_string(),
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
    fn test_extract_rust_code() {
        let response = r#"
STEP 1: Check data types
STEP 2: Verify arithmetic

RUST_CODE:
```rust
fn calculate_interest(principal: f64, rate: f64) -> f64 {
    principal * rate / 100.0
}
```

CONFIDENCE: 0.95
        "#;

        let (code, confidence) = parse_claude_response(response);
        assert!(code.contains("calculate_interest"));
        assert!((confidence - 0.95).abs() < 0.001);
    }

    #[test]
    fn test_extract_confidence_various_formats() {
        assert!((extract_confidence("CONFIDENCE: 0.94") - 0.94).abs() < 0.001);
        assert!((extract_confidence("CONFIDENCE: 1.0") - 1.0).abs() < 0.001);
        assert!((extract_confidence("CONFIDENCE: 0.9\n") - 0.9).abs() < 0.001);
        assert!((extract_confidence("no confidence here") - 0.5).abs() < 0.001);
    }
}
