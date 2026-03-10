// Llama 3.3 70B via Nebius API — FBA Node 3
// Model: meta-llama/Llama-3.3-70B-Instruct-fast
// Endpoint: https://api.tokenfactory.nebius.com/v1/

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::fba::FbaNode;

const NEBIUS_API_URL: &str = "https://api.tokenfactory.nebius.com/v1/chat/completions";
const MODEL: &str = "meta-llama/Llama-3.3-70B-Instruct-fast";

// ─── OpenAI-Compatible Request/Response ───────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
    temperature: f64,
}

#[derive(Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

// ─── Main Function ────────────────────────────────────────────────────────────

pub async fn call_llama(
    client: &reqwest::Client,
    api_key: &str,
    cobol_source: &str,
    k_star: usize,
) -> Result<FbaNode> {
    info!("Calling Llama-3.3-70B (Nebius) with k*={} CoT steps", k_star);

    let system_prompt = format!(
        "You are an expert COBOL-to-Rust modernization engineer. \
         Use exactly {} chain-of-thought reasoning steps before writing code. \
         Translate the COBOL program to idiomatic, production-quality Rust. \
         Preserve all business logic exactly. \
         End your response with: CONFIDENCE: <0.0-1.0> \
         where confidence reflects your certainty in the translation.",
        k_star
    );

    let user_prompt = format!(
        "Translate this COBOL program to Rust:\n\n```cobol\n{}\n```\n\n\
         Requirements:\n\
         1. Use {} reasoning steps (chain-of-thought)\n\
         2. Preserve all business logic exactly\n\
         3. Use idiomatic Rust (no unsafe, proper error handling)\n\
         4. End with CONFIDENCE: <score>\n\
         5. Wrap final Rust code in ```rust ... ``` blocks",
        cobol_source, k_star
    );

    let request = ChatRequest {
        model: MODEL.to_string(),
        messages: vec![
            Message { role: "system".to_string(), content: system_prompt },
            Message { role: "user".to_string(), content: user_prompt },
        ],
        max_tokens: 4096,
        temperature: 0.1,
    };

    let response = client
        .post(NEBIUS_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| anyhow!("Llama Nebius request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        error!("Llama Nebius HTTP {}: {}", status, body);
        return Err(anyhow!("Llama Nebius HTTP {}: {}", status, body));
    }

    let parsed: OpenAiResponse = response
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse Llama response: {}", e))?;

    let raw_text = parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .unwrap_or_default();

    let (rust_code, confidence) = parse_llama_response(&raw_text);

    info!(
        "Llama-3.3-70B (Nebius): code_len={} confidence={:.2}",
        rust_code.len(),
        confidence
    );

    Ok(FbaNode {
        node_id: "llama_3_3_70b".to_string(),
        model_name: "Llama-3.3-70B-Instruct-fast (Nebius)".to_string(),
        rust_code,
        confidence,
        cot_steps_used: k_star,
        raw_response: raw_text,
    })
}

// ─── Response Parser ──────────────────────────────────────────────────────────

fn parse_llama_response(raw: &str) -> (String, f64) {
    let rust_code = extract_rust_code(raw);
    let confidence = extract_confidence(raw);
    (rust_code, confidence)
}

fn extract_rust_code(raw: &str) -> String {
    if let Some(start) = raw.find("```rust") {
        let after = &raw[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }
    if let Some(start) = raw.find("```") {
        let after = &raw[start + 3..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }
    if let Some(pos) = raw.find("fn main") {
        return raw[pos..].trim().to_string();
    }
    raw.trim().to_string()
}

fn extract_confidence(raw: &str) -> f64 {
    if let Some(pos) = raw.to_lowercase().find("confidence:") {
        let after = &raw[pos + 11..];
        let token = after.split_whitespace().next().unwrap_or("0.5");
        let clean: String = token.chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        return clean.parse::<f64>().unwrap_or(0.5).clamp(0.0, 1.0);
    }
    0.5
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rust_code_llama_format() {
        let response = r#"
Let me translate this step by step:

Step 1: Identify variables — WS-PRINCIPAL, WS-RATE, WS-INTEREST
Step 2: Map PIC 9(7)V99 → f64
Step 3: COMPUTE → Rust arithmetic
Step 4: DISPLAY → println!
Step 5: Verify output format

```rust
fn main() {
    let ws_principal: f64 = 10000.00;
    let ws_rate: f64 = 5.50;
    let ws_interest: f64 = ws_principal * ws_rate / 100.0;
    println!("CALCULATED INTEREST: {:.2}", ws_interest);
}
```

CONFIDENCE: 0.92
        "#;

        let (code, confidence) = parse_llama_response(response);
        assert!(code.contains("fn main"));
        assert!(code.contains("ws_interest"));
        assert!((confidence - 0.92).abs() < 0.01);
    }
}