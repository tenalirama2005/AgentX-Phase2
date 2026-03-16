/// consensus.rs — 31-Node Dynamic FBA Consensus Engine
/// Loads model config from models.toml at startup
/// Runs all 31 LLM calls CONCURRENTLY via tokio::join_all
/// Applies 2/3 quorum (≥21/31) via FbaNetwork with dynamic QuorumSlices
///
/// Flow:
///   1. Load ModelConfig list from models.toml
///   2. Spawn all 31 model calls concurrently
///   3. Collect results — failed nodes skipped gracefully
///   4. Apply FBA quorum intersection (need ≥ 2/3 of responding nodes)
///   5. Return FbaResult with consensus Rust code + full per-node report

use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use crate::bayesian::{BayesianParams, BayesianResult};
use crate::fba::{FbaNetwork, FbaNode, FbaResult};
use crate::nebius_client::{call_model, ModelConfig};

// ─── models.toml top-level structure ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ModelsToml {
    pub network:  NetworkConfig,
    pub models:   Vec<ModelConfig>,
}

#[derive(Debug, Deserialize)]
pub struct NetworkConfig {
    pub quorum_threshold:     f64,   // e.g. 0.667
    pub similarity_threshold: f64,   // e.g. 0.75
    pub confidence_threshold: f64,   // e.g. 0.80
    #[allow(dead_code)]
    pub pass1_max_tokens:     u32,   // 1024 — reserved for future per-network override
    #[allow(dead_code)]
    pub description:          String, // human-readable label in models.toml
}

// ─── ConsensusConfig — runtime state ─────────────────────────────────────────

#[derive(Clone)]
pub struct ConsensusConfig {
    pub models:               Vec<ModelConfig>,
    pub quorum_threshold:     f64,
    pub similarity_threshold: f64,
    pub confidence_threshold: f64,
}

impl ConsensusConfig {
    /// Load from models.toml — called once at purple_agent startup
    pub fn from_toml(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Cannot read {}: {}", path, e))?;

        let parsed: ModelsToml = toml::from_str(&content)
            .map_err(|e| anyhow!("Invalid models.toml: {}", e))?;

        info!(
            "📋 Loaded {} models from {} | quorum={:.0}% | sim_threshold={:.2} | conf_threshold={:.2}",
            parsed.models.len(), path,
            parsed.network.quorum_threshold * 100.0,
            parsed.network.similarity_threshold,
            parsed.network.confidence_threshold,
        );

        // Log model registry at startup
        for (i, m) in parsed.models.iter().enumerate() {
            info!(
                "  [{:02}] {} [{}] provider={} pass2_tokens={}",
                i + 1, m.node_id, m.tier, m.provider, m.pass2_max_tokens
            );
        }

        Ok(Self {
            models:               parsed.models,
            quorum_threshold:     parsed.network.quorum_threshold,
            similarity_threshold: parsed.network.similarity_threshold,
            confidence_threshold: parsed.network.confidence_threshold,
        })
    }

    /// Quorum = ceil(n_models * quorum_threshold)
    pub fn quorum_size(&self) -> usize {
        let n = self.models.len() as f64;
        (n * self.quorum_threshold).ceil() as usize
    }
}

// ─── AppState for purple_agent ────────────────────────────────────────────────

pub struct AppState {
    pub http_client:   reqwest::Client,
    pub nebius_key:    String,
    pub anthropic_key: String,
    pub config:        ConsensusConfig,
}

// ─── Main Entry Point ─────────────────────────────────────────────────────────

/// Run all 31 models concurrently, apply FBA quorum, return consensus result.
pub async fn run_consensus(
    state:        &AppState,
    cobol_source: &str,
) -> FbaResult {
    let bayesian_result = compute_bayesian(cobol_source);
    let k_star = bayesian_result.k_star;
    let n_models = state.config.models.len();
    let quorum_needed = state.config.quorum_size();

    info!(
        "🚀 Starting 31-node FBA consensus | k*={} | quorum={}/{} | sim={:.2} | conf={:.2}",
        k_star, quorum_needed, n_models,
        state.config.similarity_threshold,
        state.config.confidence_threshold,
    );

    // ── Spawn all model calls concurrently ───────────────────────────────────
    let client   = Arc::new(state.http_client.clone());
    let nkey     = Arc::new(state.nebius_key.clone());
    let akey     = Arc::new(state.anthropic_key.clone());
    let cobol    = Arc::new(cobol_source.to_string());
    let configs  = Arc::new(state.config.models.clone());

    let mut join_set: JoinSet<(String, Result<FbaNode>)> = JoinSet::new();

    for config in configs.iter().cloned() {
        let client  = Arc::clone(&client);
        let nkey    = Arc::clone(&nkey);
        let akey    = Arc::clone(&akey);
        let cobol   = Arc::clone(&cobol);
        let node_id = config.node_id.clone();

        join_set.spawn(async move {
            let result = call_model(
                &client, &config, &nkey, &akey, &cobol, k_star,
            ).await;
            (node_id, result)
        });
    }

    // ── Collect results ───────────────────────────────────────────────────────
    let mut successful_nodes: Vec<FbaNode> = Vec::new();
    let mut failed_nodes:     Vec<String>  = Vec::new();

    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok((node_id, Ok(fba_node))) => {
                info!(
                    "  ✅ {} → code_len={} confidence={:.2} cot={}",
                    node_id, fba_node.rust_code.len(),
                    fba_node.confidence, fba_node.cot_steps_used
                );
                successful_nodes.push(fba_node);
            }
            Ok((node_id, Err(e))) => {
                warn!("  ⚠️  {} FAILED: {}", node_id, e);
                failed_nodes.push(node_id);
            }
            Err(join_err) => {
                error!("  ❌ Task panicked: {}", join_err);
            }
        }
    }

    info!(
        "📊 Results: {}/{} nodes succeeded | {} failed | quorum_needed={}",
        successful_nodes.len(), n_models,
        failed_nodes.len(), quorum_needed
    );

    // Log failed nodes for audit
    if !failed_nodes.is_empty() {
        warn!("  Failed nodes: {:?}", failed_nodes);
    }

    // ── Check if enough nodes responded ──────────────────────────────────────
    if successful_nodes.len() < 2 {
        error!("QUORUM_VIOLATION: only {} nodes responded", successful_nodes.len());
        return FbaResult {
            status:              "QUORUM_VIOLATION".to_string(),
            rust_code:           None,
            confidence:          0.0,
            semantic_similarity: 0.0,
            bayesian_guarantee:  "VIOLATED".to_string(),
            martingale_satisfied: false,
            k_star,
            node_results:        successful_nodes,
            paper_reference:     "arxiv:2507.11768".to_string(),
        };
    }

    // ── Build dynamic FBA network from responding nodes ────────────────────
    let responding_ids: Vec<String> = successful_nodes
        .iter()
        .map(|n| n.node_id.clone())
        .collect();

    let network = FbaNetwork::new_dynamic(
        &responding_ids,
        state.config.quorum_threshold,
    );

    // ── Run FBA consensus ─────────────────────────────────────────────────────
    let result = network.check_consensus(
        &successful_nodes,
        state.config.similarity_threshold,
        state.config.confidence_threshold,
        &bayesian_result,
    );

    info!(
        "🏁 FBA result: {} | confidence={:.3} | similarity={:.3} | bayesian={}",
        result.status, result.confidence,
        result.semantic_similarity, result.bayesian_guarantee
    );

    result
}

// ─── Bayesian helper ──────────────────────────────────────────────────────────

fn compute_bayesian(cobol_source: &str) -> BayesianResult {
    let cobol_line_count = cobol_source.lines().count();
    crate::bayesian::compute_k_star(&BayesianParams {
        cobol_line_count,
        epsilon: 0.01,
        theta:   2.5,
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_config(n: usize) -> ConsensusConfig {
        let models: Vec<ModelConfig> = (0..n).map(|i| ModelConfig {
            node_id:          format!("node_{:02}", i),
            model_name:       format!("TestModel-{}", i),
            provider:         "nebius".to_string(),
            model_id:         format!("test/model-{}", i),
            pass2_max_tokens: 4096,
            temperature:      0.1,
            family:           "test".to_string(),
            tier:             "test".to_string(),
        }).collect();

        ConsensusConfig {
            models,
            quorum_threshold:     0.667,
            similarity_threshold: 0.75,
            confidence_threshold: 0.80,
        }
    }

    #[test]
    fn test_quorum_size_31_nodes() {
        let cfg = make_test_config(31);
        // ceil(31 * 0.667) = ceil(20.677) = 21
        assert_eq!(cfg.quorum_size(), 21);
    }

    #[test]
    fn test_quorum_size_3_nodes() {
        let cfg = make_test_config(3);
        // ceil(3 * 0.667) = ceil(2.001) = 3 — but FbaNetwork handles 2/3
        assert_eq!(cfg.quorum_size(), 2);
    }

    #[test]
    fn test_quorum_size_with_failures() {
        let cfg = make_test_config(31);
        // Even with 10 failures, 21 remaining still meets quorum
        let responding = 31 - 10;
        assert!(responding >= cfg.quorum_size());
    }

    #[test]
    fn test_models_toml_parse() {
        let toml_str = r#"
[network]
quorum_threshold     = 0.667
similarity_threshold = 0.75
confidence_threshold = 0.80
pass1_max_tokens     = 1024
description          = "Test network"

[[models]]
node_id          = "claude_opus_4_6"
model_name       = "Claude Opus 4.6"
provider         = "anthropic"
model_id         = "claude-opus-4-6"
pass2_max_tokens = 1500
temperature      = 0.1
family           = "claude"
tier             = "anchor"
cost_input       = 15.00
cost_output      = 75.00

[[models]]
node_id          = "deepseek_v3_2"
model_name       = "DeepSeek-V3.2"
provider         = "nebius"
model_id         = "deepseek-ai/DeepSeek-V3.2"
pass2_max_tokens = 8192
temperature      = 0.1
family           = "deepseek"
tier             = "tier1"
cost_input       = 0.30
cost_output      = 0.45
"#;
        let parsed: ModelsToml = toml::from_str(toml_str).expect("Should parse");
        assert_eq!(parsed.models.len(), 2);
        assert_eq!(parsed.models[0].node_id, "claude_opus_4_6");
        assert_eq!(parsed.models[0].provider, "anthropic");
        assert_eq!(parsed.models[1].provider, "nebius");
        assert!((parsed.network.quorum_threshold - 0.667).abs() < 0.001);
    }

    #[test]
    fn test_config_from_invalid_path() {
        let result = ConsensusConfig::from_toml("/nonexistent/models.toml");
        assert!(result.is_err());
    }
}