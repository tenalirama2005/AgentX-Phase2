/// Consensus orchestrator
/// Ties together Bayesian k* computation, parallel LLM calls,
/// and FBA quorum intersection into a single pipeline.
use crate::{
    bayesian::{compute_k_star, count_cobol_lines, BayesianParams, BayesianResult},
    claude::call_claude,
    deepseek_v3::call_deepseek_v3,
    fba::{run_fba_consensus, FbaConsensusResult, FbaNetwork},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

/// Configuration for the consensus pipeline
#[derive(Debug, Clone)]
pub struct ConsensusConfig {
    /// Minimum semantic similarity to accept consensus [0.0, 1.0]
    pub similarity_threshold: f64,
    /// Minimum confidence score per LLM node [0.0, 1.0]
    pub confidence_threshold: f64,
    /// Error tolerance ε for k* formula
    pub epsilon: f64,
    /// Θ scaling constant for k* formula
    pub theta: f64,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.75,
            confidence_threshold: 0.85,
            epsilon: 0.01,
            theta: 2.5,
        }
    }
}

/// Full modernization request
#[derive(Debug, Deserialize)]
pub struct ModernizeRequest {
    /// COBOL source code to translate
    pub cobol_source: String,
    /// Optional override for epsilon
    pub epsilon: Option<f64>,
    /// Optional override for similarity threshold
    pub similarity_threshold: Option<f64>,
}

/// Full modernization response — the AgentX-winning output
#[derive(Debug, Serialize)]
pub struct ModernizeResponse {
    /// Unique request ID for audit trail
    pub request_id: String,
    /// CONSENSUS_REACHED | DISAGREEMENT | QUORUM_VIOLATION
    pub status: String,
    /// Translated Rust code (only present on consensus)
    pub rust_code: Option<String>,
    /// Combined FBA confidence
    pub confidence: f64,
    /// Bayesian-in-Realization or in-Expectation-Only
    pub bayesian_guarantee: String,
    /// k* value used
    pub k_star: usize,
    /// Semantic similarity between Claude and Deep Seek V3
    pub semantic_similarity: f64,
    /// Martingale property satisfied?
    pub martingale_satisfied: bool,
    /// Bayesian computation details
    pub bayesian_details: BayesianResult,
    /// Full FBA consensus details
    pub fba_details: FbaConsensusResult,
    /// Paper reference
    pub paper_reference: String,
    /// Action taken
    pub action: String,
}

/// Run the complete consensus pipeline:
/// 1. Compute k* (Bayesian optimal CoT)
/// 2. Call Claude + Deep Seek V3 in parallel
/// 3. Run FBA consensus
/// 4. Return verdict + code
pub async fn run_consensus_pipeline(
    client: &reqwest::Client,
    anthropic_key: &str,
    openai_key: &str,
    request: ModernizeRequest,
    config: &ConsensusConfig,
) -> Result<ModernizeResponse> {
    let request_id = Uuid::new_v4().to_string();
    info!("Starting consensus pipeline request_id={}", request_id);

    // ── Step 1: Compute k* ────────────────────────────────────────────────────
    let cobol_lines = count_cobol_lines(&request.cobol_source);
    let params = BayesianParams {
        cobol_line_count: cobol_lines.max(10),
        epsilon: request.epsilon.unwrap_or(config.epsilon),
        theta: config.theta,
    };
    let bayesian = compute_k_star(&params);
    let k_star = bayesian.k_star;

    info!(
        "k* computed: {} | formula: {} | coverage: {:.2}%",
        k_star, bayesian.formula, bayesian.entropy_coverage
    );

    let similarity_threshold = request
        .similarity_threshold
        .unwrap_or(config.similarity_threshold);

    // ── Step 2: Parallel LLM calls ────────────────────────────────────────────
    info!("Firing parallel LLM calls (Claude + Deep Seek V3)...");

    let (claude_result, deepseek_v3_result) = tokio::join!(
        call_claude(client, anthropic_key, &request.cobol_source, k_star),
        call_deepseek_v3(client, openai_key, &request.cobol_source, k_star),
    );

    let mut nodes = Vec::new();

    match claude_result {
        Ok(node) => {
            info!(
                "Claude ✅ confidence={:.3} code_len={}",
                node.confidence,
                node.rust_code.len()
            );
            nodes.push(node);
        }
        Err(e) => {
            warn!("Claude ❌ failed: {}", e);
        }
    }

    match deepseek_v3_result {
        Ok(node) => {
            info!(
                "Deep Seek V3 ✅ confidence={:.3} code_len={}",
                node.confidence,
                node.rust_code.len()
            );
            nodes.push(node);
        }
        Err(e) => {
            warn!("Deep Seek V3 ❌ failed: {}", e);
        }
    }

    // ── Step 3: FBA Consensus ─────────────────────────────────────────────────
    let network = FbaNetwork::two_node_network();
    let fba_result = run_fba_consensus(
        nodes,
        &network,
        similarity_threshold,
        config.confidence_threshold,
    );

    // ── Step 4: Determine action ──────────────────────────────────────────────
    let action = match fba_result.verdict.as_str() {
        "CONSENSUS_REACHED" => {
            info!(
                "✅ CONSENSUS REACHED | confidence={:.3} | similarity={:.3}",
                fba_result.confidence, fba_result.semantic_similarity
            );
            "SAVE_TO_S3".to_string()
        }
        "DISAGREEMENT" => {
            warn!(
                "⚠️  DISAGREEMENT | similarity={:.3} | human review required",
                fba_result.semantic_similarity
            );
            "HUMAN_REVIEW".to_string()
        }
        _ => {
            warn!("🚨 QUORUM VIOLATION — system fault");
            "SYSTEM_FAULT".to_string()
        }
    };

    Ok(ModernizeResponse {
        request_id,
        status: fba_result.verdict.clone(),
        rust_code: fba_result.rust_code.clone(),
        confidence: fba_result.confidence,
        bayesian_guarantee: fba_result.bayesian_guarantee.clone(),
        k_star,
        semantic_similarity: fba_result.semantic_similarity,
        martingale_satisfied: fba_result.martingale_satisfied,
        bayesian_details: bayesian,
        fba_details: fba_result,
        paper_reference: "arxiv:2507.11768".to_string(),
        action,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_config_defaults() {
        let cfg = ConsensusConfig::default();
        assert_eq!(cfg.epsilon, 0.01);
        assert_eq!(cfg.similarity_threshold, 0.75);
        assert_eq!(cfg.confidence_threshold, 0.85);
    }
}
