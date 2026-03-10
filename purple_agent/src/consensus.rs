// FBA Consensus Orchestrator — purple_agent v0.3.0
// 3-Node FBA: Claude Opus 4.6 + DeepSeek V3.2 + Llama 3.3 70B
// All via Nebius API (DeepSeek + Llama) and Anthropic API (Claude)
// arxiv:2507.11768 — k* = Θ(√n × log(1/ε))

use anyhow::Result;
use tracing::{info, warn};
use uuid::Uuid;

use crate::bayesian::{compute_k_star, BayesianParams};
use crate::claude::call_claude;
use crate::deepseek_v3::call_deepseek;
use crate::fba::{FbaNetwork, FbaNode, FbaResult};
use crate::llama::call_llama;

// ─── Consensus Config ─────────────────────────────────────────────────────────

pub struct ConsensusConfig {
    pub similarity_threshold: f64,
    pub confidence_threshold: f64,
    pub epsilon: f64,
    pub theta: f64,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: std::env::var("SIMILARITY_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.75),
            confidence_threshold: std::env::var("CONFIDENCE_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.85),
            epsilon: std::env::var("BAYESIAN_EPSILON")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.01),
            theta: std::env::var("BAYESIAN_THETA")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2.5),
        }
    }
}

// ─── Pipeline ─────────────────────────────────────────────────────────────────

/// Run full 3-node FBA consensus pipeline
/// Returns FbaResult with consensus status and verified Rust code
pub async fn run_consensus(
    http_client: &reqwest::Client,
    anthropic_key: &str,
    nebius_key: &str,
    cobol_source: &str,
    config: &ConsensusConfig,
) -> Result<FbaResult> {
    let request_id = Uuid::new_v4().to_string();
    info!("🟣 FBA Consensus v0.3.0 starting — request_id={}", request_id);

    // ── Step 1: Compute k* (Bayesian optimal CoT length) ─────────────────────
    let line_count = cobol_source.lines().count();
    let params = BayesianParams {
        cobol_line_count: line_count,
        epsilon: config.epsilon,
        theta: config.theta,
    };
    let bayesian = compute_k_star(&params);
    let k_star = bayesian.k_star;

    info!(
        "Bayesian k*={} for {} COBOL lines | formula={} | entropy={:.1}%",
        k_star, line_count, bayesian.formula, bayesian.entropy_coverage
    );

    // ── Step 2: Call all 3 nodes in parallel ─────────────────────────────────
    info!("🚀 Calling 3 FBA nodes in parallel...");
    info!("   Node 1: Claude Opus 4.6 (Anthropic)");
    info!("   Node 2: DeepSeek V3.2 (Nebius)");
    info!("   Node 3: Llama-3.3-70B (Nebius)");

    let (claude_result, deepseek_result, llama_result) = tokio::join!(
        call_claude(http_client, anthropic_key, cobol_source, k_star),
        call_deepseek(http_client, nebius_key, cobol_source, k_star),
        call_llama(http_client, nebius_key, cobol_source, k_star),
    );

    // ── Step 3: Collect successful nodes ─────────────────────────────────────
    let mut nodes: Vec<FbaNode> = Vec::new();

    match claude_result {
        Ok(node) => {
            info!("✅ Node 1 (Claude): confidence={:.3}", node.confidence);
            nodes.push(node);
        }
        Err(e) => warn!("⚠️ Node 1 (Claude) failed: {}", e),
    }

    match deepseek_result {
        Ok(node) => {
            info!("✅ Node 2 (DeepSeek): confidence={:.3}", node.confidence);
            nodes.push(node);
        }
        Err(e) => warn!("⚠️ Node 2 (DeepSeek) failed: {}", e),
    }

    match llama_result {
        Ok(node) => {
            info!("✅ Node 3 (Llama): confidence={:.3}", node.confidence);
            nodes.push(node);
        }
        Err(e) => warn!("⚠️ Node 3 (Llama) failed: {}", e),
    }

    // Need at least 2 nodes for quorum
    if nodes.len() < 2 {
        return Ok(FbaResult {
            status: "QUORUM_VIOLATION".to_string(),
            rust_code: None,
            confidence: 0.0,
            semantic_similarity: 0.0,
            bayesian_guarantee: "FAILED".to_string(),
            martingale_satisfied: false,
            k_star,
            node_results: nodes,
            paper_reference: "arxiv:2507.11768".to_string(),
        });
    }

    // ── Step 4: FBA Quorum Intersection ──────────────────────────────────────
    let network = FbaNetwork::new_three_node();
    let fba_result = network.check_consensus(
        &nodes,
        config.similarity_threshold,
        config.confidence_threshold,
        &bayesian,
    );

    info!(
        "🏁 FBA result: {} | confidence={:.3} | similarity={:.3} | k*={} | bayesian={}",
        fba_result.status,
        fba_result.confidence,
        fba_result.semantic_similarity,
        fba_result.k_star,
        fba_result.bayesian_guarantee
    );

    Ok(fba_result)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_config_defaults() {
        let config = ConsensusConfig::default();
        assert!(config.similarity_threshold > 0.0);
        assert!(config.confidence_threshold > 0.0);
        assert!(config.epsilon > 0.0);
        assert!(config.theta > 0.0);
    }
}