/// nebius_client.rs — Generic Two-Pass LLM Client
/// Handles ALL 30 Nebius models + Claude (Anthropic) via unified ModelConfig
/// Loaded dynamically from models.toml — no hardcoded model logic
///
/// Two-Pass Architecture (same proven pattern from Sprint 2):
///   Pass 1: Reasoning only    — max_tokens=1024, "Do NOT write code yet"
///   Pass 2: Code generation   — max_tokens=config.pass2_max_tokens, concise Rust
///
/// Provider routing:
///   provider="anthropic" → api.anthropic.com  (Claude Opus 4.6)
///   provider="nebius"    → api.tokenfactory.nebius.com  (all 30 others)

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn, error};

use crate::fba::FbaNode;

// ─── Constants ────────────────────────────────────────────────────────────────

const NEBIUS_API_URL: &str  = "https://api.tokenfactory.nebius.com/v1/chat/completions";
const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

// ─── Model Config (matches models.toml [[models]] entries) ────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub node_id:          String,
    pub model_name:       String,
    pub provider:         String,   // "anthropic" | "nebius"
    pub model_id:         String,   // exact API model string
    pub pass2_max_tokens: u32,
    pub temperature:      f64,
    pub family:           String,
    pub tier:             String,
}

// ─── Nebius / OpenAI-compatible types ─────────────────────────────────────────

#[derive(Serialize, Clone)]
struct ChatMessage {
    role:    String,
    content: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model:       String,
    messages:    Vec<ChatMessage>,
    max_tokens:  u32,
    temperature: f64,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

#[derive(Deserialize, Default)]
struct ChatUsage {
    prompt_tokens:     u32,
    completion_tokens: u32,
}

// ─── Anthropic types ──────────────────────────────────────────────────────────

#[derive(Serialize)]
struct AnthropicRequest {
    model:       String,
    max_tokens:  u32,
    temperature: f64,
    system:      String,
    messages:    Vec<AnthropicMessage>,
}

#[derive(Serialize, Clone)]
struct AnthropicMessage {
    role:    String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text:         Option<String>,
}

// ─── System Prompts (shared across all models) ────────────────────────────────

fn reasoning_system_prompt() -> &'static str {
    "You are an expert COBOL-to-Rust modernization engineer. \
     Analyze the given COBOL program carefully. \
     Think through: data types, numeric precision, business logic, I/O patterns. \
     Do NOT write any Rust code yet — reasoning only."
}

fn codegen_system_prompt() -> &'static str {
    "You are an expert COBOL-to-Rust modernization engineer. \
     Write a CONCISE, production-quality Rust translation. \
     Rules:\
     \n1. Preserve ALL business logic exactly\
     \n2. Match numeric precision (use f64 for decimal calculations)\
     \n3. Use descriptive variable names matching COBOL data names where possible\
     \n4. ONE brief doc comment only — no inline comments unless critical\
     \n5. Code must compile with `cargo build` without errors\
     \n6. Do NOT add extra helper functions, structs, or explanatory text beyond what the COBOL requires\
     \n7. Do NOT truncate\
     \n8. For CONFIDENCE: report calibrated certainty — a straightforward translation \
with no ambiguity warrants 0.92 or higher. \
Output ONLY a decimal number after CONFIDENCE: with no other text on that line"
}

// ─── Nebius API call (OpenAI-compatible) ──────────────────────────────────────

async fn call_nebius(
    client:      &reqwest::Client,
    api_key:     &str,
    model_id:    &str,
    messages:    Vec<ChatMessage>,
    max_tokens:  u32,
    temperature: f64,
) -> Result<String> {
    let body = ChatRequest {
        model: model_id.to_string(),
        messages,
        max_tokens,
        temperature,
    };

    let response = client
        .post(NEBIUS_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("Nebius API request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        error!("Nebius API error {}: {}", status, text);
        return Err(anyhow!("Nebius API {} for model {}: {}", status, model_id, text));
    }

    let parsed: ChatResponse = response
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse Nebius response: {}", e))?;

    if let Some(usage) = &parsed.usage {
        info!(
            "Nebius [{}] tokens: prompt={} completion={}",
            model_id, usage.prompt_tokens, usage.completion_tokens
        );
    }

    if let Some(choice) = parsed.choices.first() {
        if let Some(reason) = &choice.finish_reason {
            if reason == "length" {
                warn!("Nebius [{}] finish_reason=length — may be truncated", model_id);
            }
        }
        Ok(choice.message.content.clone())
    } else {
        Err(anyhow!("Nebius returned no choices for model {}", model_id))
    }
}

// ─── Anthropic API call ───────────────────────────────────────────────────────

async fn call_anthropic(
    client:      &reqwest::Client,
    api_key:     &str,
    model_id:    &str,
    system:      &str,
    messages:    Vec<AnthropicMessage>,
    max_tokens:  u32,
    temperature: f64,
) -> Result<String> {
    let body = AnthropicRequest {
        model: model_id.to_string(),
        max_tokens,
        temperature,
        system: system.to_string(),
        messages,
    };

    let response = client
        .post(ANTHROPIC_API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("Anthropic API request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        error!("Anthropic API error {}: {}", status, text);
        return Err(anyhow!("Anthropic API {}: {}", status, text));
    }

    let parsed: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse Anthropic response: {}", e))?;

    let text = parsed
        .content
        .iter()
        .filter(|c| c.content_type == "text")
        .filter_map(|c| c.text.as_deref())
        .collect::<Vec<_>>()
        .join("");

    Ok(text)
}

// ─── Confidence Extractor (3-strategy, fallback 0.85) ────────────────────────

pub fn extract_confidence(text: &str) -> f64 {
    // Strategy 1: "CONFIDENCE: 0.92" or "CONFIDENCE: 95"
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("confidence:") {
            let after = lower.split("confidence:").nth(1).unwrap_or("").trim().to_string();
            let num_str: String = after
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(v) = num_str.parse::<f64>() {
                let confidence = if v > 1.0 { v / 100.0 } else { v };
                if (0.0..=1.0).contains(&confidence) {
                    return confidence;
                }
            }
        }
    }

    // Strategy 2: scan for decimal like 0.92 or 0.88 near end of text
    let last_500 = if text.len() > 500 { &text[text.len() - 500..] } else { text };
    for word in last_500.split_whitespace().rev() {
        let clean: String = word.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
        if let Ok(v) = clean.parse::<f64>() {
            if (0.5..=1.0).contains(&v) {
                return v;
            }
        }
    }

    // Strategy 3: percentage like "92%"
    for word in text.split_whitespace() {
        if word.ends_with('%') {
            let num: String = word.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(v) = num.parse::<f64>() {
                let confidence = v / 100.0;
                if (0.5..=1.0).contains(&confidence) {
                    return confidence;
                }
            }
        }
    }

    // Fallback: calibrated default
    warn!("Could not extract confidence — using fallback 0.85");
    0.85
}

// ─── Code Extractor ───────────────────────────────────────────────────────────

fn extract_rust_code(text: &str) -> String {
    // Try ```rust ... ``` block first
    if let Some(start) = text.find("```rust") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }
    // Try ``` ... ``` block
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        // Skip language tag if any
        let code_start = after.find('\n').map(|i| i + 1).unwrap_or(0);
        if let Some(end) = after[code_start..].find("```") {
            return after[code_start..code_start + end].trim().to_string();
        }
    }
    // Try RUST_CODE: marker
    if let Some(start) = text.find("RUST_CODE:") {
        return text[start + 10..].trim().to_string();
    }
    // Fallback: entire response
    text.trim().to_string()
}

// ─── Main Public Entry Point ──────────────────────────────────────────────────

/// Call a single model (Nebius or Anthropic) using two-pass architecture.
/// Returns FbaNode with rust_code, confidence, cot_steps_used.
pub async fn call_model(
    client:       &reqwest::Client,
    config:       &ModelConfig,
    nebius_key:   &str,
    anthropic_key: &str,
    cobol_source: &str,
    k_star:       usize,
) -> Result<FbaNode> {
    let effective_steps = k_star.min(20);

    info!(
        "🤖 [{}/{}/{}] Starting two-pass inference (steps={})",
        config.node_id, config.family, config.tier, effective_steps
    );

    let pass1_user = format!(
        "Analyze this COBOL program in {} reasoning steps. \
         Focus on: data types, numeric precision, business logic, I/O.\n\n\
         COBOL SOURCE:\n```cobol\n{}\n```",
        effective_steps, cobol_source
    );

    let pass2_user = format!(
        "Now write the Rust translation based on your analysis above.\n\
         Target length: 800-1500 chars.\n\
         Be concise — no helper functions, no extra structs, no verbose comments.\n\
         Output format:\n\n\
         RUST_CODE:\n\
         ```rust\n\
         [concise Rust implementation]\n\
         ```\n\n\
         CONFIDENCE: [decimal 0.0-1.0 only, no other text on this line]\n\
         Note: For a straightforward COBOL translation with no ambiguity, \
confidence should be 0.90 or higher."
    );

    // ── Dispatch by provider ──────────────────────────────────────────────────
    let raw_text = match config.provider.as_str() {
        "anthropic" => {
            call_anthropic_two_pass(
                client, anthropic_key, &config.model_id,
                &pass1_user, &pass2_user, config.pass2_max_tokens, config.temperature,
            ).await?
        }
        "nebius" => {
            call_nebius_two_pass(
                client, nebius_key, &config.model_id,
                &pass1_user, &pass2_user, config.pass2_max_tokens, config.temperature,
            ).await?
        }
        other => return Err(anyhow!("Unknown provider: {}", other)),
    };

    let rust_code  = extract_rust_code(&raw_text);
    let confidence = extract_confidence(&raw_text);

    info!(
        "✅ [{}/{}/{}] code_len={} confidence={:.2}",
        config.node_id, config.family, config.tier, rust_code.len(), confidence
    );

    Ok(FbaNode {
        node_id:       config.node_id.clone(),
        model_name:    config.model_name.clone(),
        rust_code,
        confidence,
        cot_steps_used: effective_steps,
        raw_response:  raw_text,
    })
}

// ─── Two-Pass: Nebius ─────────────────────────────────────────────────────────

async fn call_nebius_two_pass(
    client:      &reqwest::Client,
    api_key:     &str,
    model_id:    &str,
    pass1_user:  &str,
    pass2_user:  &str,
    pass2_max:   u32,
    temperature: f64,
) -> Result<String> {
    // Pass 1 — reasoning only
    let p1_messages = vec![
        ChatMessage { role: "system".into(), content: reasoning_system_prompt().into() },
        ChatMessage { role: "user".into(),   content: pass1_user.into() },
    ];

    info!("  [{model_id}] Pass 1: reasoning (max_tokens=1024)");
    let reasoning = call_nebius(client, api_key, model_id, p1_messages, 1024, temperature)
        .await
        .unwrap_or_else(|e| {
            warn!("  [{model_id}] Pass 1 failed: {} — using empty reasoning", e);
            String::new()
        });

    // Pass 2 — code generation, inject reasoning as assistant turn
    let p2_messages = vec![
        ChatMessage { role: "system".into(),    content: codegen_system_prompt().into() },
        ChatMessage { role: "user".into(),       content: pass1_user.into() },
        ChatMessage { role: "assistant".into(),  content: reasoning.clone() },
        ChatMessage { role: "user".into(),       content: pass2_user.into() },
    ];

    info!("  [{model_id}] Pass 2: codegen (max_tokens={pass2_max})");
    call_nebius(client, api_key, model_id, p2_messages, pass2_max, temperature).await
}

// ─── Two-Pass: Anthropic ──────────────────────────────────────────────────────

async fn call_anthropic_two_pass(
    client:      &reqwest::Client,
    api_key:     &str,
    model_id:    &str,
    pass1_user:  &str,
    pass2_user:  &str,
    pass2_max:   u32,
    temperature: f64,
) -> Result<String> {
    // Pass 1 — reasoning only
    let p1_messages = vec![
        AnthropicMessage { role: "user".into(), content: pass1_user.into() },
    ];

    info!("  [{model_id}] Pass 1: reasoning (max_tokens=1024)");
    let reasoning = call_anthropic(
        client, api_key, model_id,
        reasoning_system_prompt(), p1_messages, 1024, temperature,
    ).await.unwrap_or_else(|e| {
        warn!("  [{model_id}] Pass 1 failed: {} — using empty reasoning", e);
        String::new()
    });

    // Pass 2 — inject reasoning as assistant turn
    let p2_messages = vec![
        AnthropicMessage { role: "user".into(),      content: pass1_user.into() },
        AnthropicMessage { role: "assistant".into(), content: reasoning.clone() },
        AnthropicMessage { role: "user".into(),      content: pass2_user.into() },
    ];

    info!("  [{model_id}] Pass 2: codegen (max_tokens={pass2_max})");
    call_anthropic(
        client, api_key, model_id,
        codegen_system_prompt(), p2_messages, pass2_max, temperature,
    ).await
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_confidence_decimal() {
        assert!((extract_confidence("CONFIDENCE: 0.92") - 0.92).abs() < 0.001);
    }

    #[test]
    fn test_extract_confidence_percentage() {
        assert!((extract_confidence("CONFIDENCE: 95") - 0.95).abs() < 0.001);
    }

    #[test]
    fn test_extract_confidence_with_trailing_text() {
        assert!((extract_confidence("CONFIDENCE: 0.91 (high certainty)") - 0.91).abs() < 0.001);
    }

    #[test]
    fn test_extract_confidence_case_insensitive() {
        assert!((extract_confidence("confidence: 0.88") - 0.88).abs() < 0.001);
    }

    #[test]
    fn test_extract_confidence_fallback() {
        assert!((extract_confidence("no score here") - 0.85).abs() < 0.001);
    }

    #[test]
    fn test_extract_rust_code_backtick_block() {
        let text = "Here is the code:\n```rust\nfn main() {}\n```\nCONFIDENCE: 0.92";
        assert_eq!(extract_rust_code(text), "fn main() {}");
    }

    #[test]
    fn test_extract_rust_code_plain_block() {
        let text = "```\nfn foo() -> i32 { 42 }\n```";
        assert_eq!(extract_rust_code(text), "fn foo() -> i32 { 42 }");
    }

    #[test]
    fn test_extract_rust_code_fallback() {
        let text = "fn main() {}";
        assert!(extract_rust_code(text).contains("fn main()"));
    }
}
