/// Federated Byzantine Agreement (FBA) Engine
/// Adapted from Stellar Consensus Protocol for LLM consensus
/// Based on: arxiv:2507.11768 — "LLMs are Bayesian, in Expectation, not in Realization"
///
/// FBA enables a set of LLM "nodes" to reach agreement without
/// trusting a central authority. Each node has a quorum slice —
/// a set of nodes whose agreement is sufficient for that node
/// to feel "certain."
///
/// For 3-LLM setup (Claude + DeepSeek V3.2 + Llama-3.3-70B):
///   Quorum = any 2 of 3 nodes agree
///   Quorum slice for each = any 1 of the other 2 nodes
///   Intersection guaranteed when 2/3 agree → martingale restored

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{info, warn};

/// An FBA "node" representing one LLM participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FbaNode {
    pub node_id: String,
    pub model_name: String,
    pub rust_code: String,
    pub confidence: f64,
    pub cot_steps_used: usize,
    pub raw_response: String,
}

/// Quorum slice: the set of nodes that together form a quorum for a given node
#[derive(Debug, Clone)]
pub struct QuorumSlice {
    #[allow(dead_code)]
    pub node_id: String,
    pub trusted_nodes: Vec<String>,
    /// Threshold: how many trusted nodes must agree for this node to ratify
    pub threshold: usize,
}

impl QuorumSlice {
    /// Check if this node's quorum slice is satisfied given a set of agreeing node IDs.
    /// Per SCP: a node v ratifies a statement if enough of its trusted nodes agree.
    pub fn is_satisfied(&self, agreeing_nodes: &[&str]) -> bool {
        let count = self.trusted_nodes
            .iter()
            .filter(|trusted| agreeing_nodes.contains(&trusted.as_str()))
            .count();
        count >= self.threshold
    }
}

/// FBA Network configuration
#[derive(Debug, Clone)]
pub struct FbaNetwork {
    #[allow(dead_code)]
    pub nodes: Vec<String>,
    pub quorum_slices: HashMap<String, QuorumSlice>,
    /// Global quorum threshold (fraction of total nodes)
    #[allow(dead_code)]
    pub quorum_threshold: f64,
}

impl FbaNetwork {
    /// Create a 3-node FBA network (Claude + DeepSeek V3.2 + Llama 3.3 70B)
    /// Quorum = any 2 of 3 nodes agree → quorum intersection guaranteed
    #[allow(dead_code)]
    pub fn new_three_node() -> Self {
        let nodes = vec![
            "claude_opus_4_6".to_string(),
            "deepseek_v3_nebius".to_string(),
            "llama_3_3_70b".to_string(),
        ];

        let mut quorum_slices = HashMap::new();

        quorum_slices.insert(
            "claude_opus_4_6".to_string(),
            QuorumSlice {
                node_id: "claude_opus_4_6".to_string(),
                trusted_nodes: vec![
                    "deepseek_v3_nebius".to_string(),
                    "llama_3_3_70b".to_string(),
                ],
                threshold: 1,   // needs 1 of 2 trusted nodes to agree
            },
        );

        quorum_slices.insert(
            "deepseek_v3_nebius".to_string(),
            QuorumSlice {
                node_id: "deepseek_v3_nebius".to_string(),
                trusted_nodes: vec![
                    "claude_opus_4_6".to_string(),
                    "llama_3_3_70b".to_string(),
                ],
                threshold: 1,
            },
        );

        quorum_slices.insert(
            "llama_3_3_70b".to_string(),
            QuorumSlice {
                node_id: "llama_3_3_70b".to_string(),
                trusted_nodes: vec![
                    "claude_opus_4_6".to_string(),
                    "deepseek_v3_nebius".to_string(),
                ],
                threshold: 1,
            },
        );

        Self {
            nodes,
            quorum_slices,
            quorum_threshold: 0.667, // 2/3 nodes must agree
        }
    }

    /// Create an N-node FBA network dynamically from responding node IDs.
    /// Each node trusts all others — threshold = ceil(n * quorum_threshold) - 1
    /// This allows graceful degradation when some nodes fail or timeout.
    pub fn new_dynamic(node_ids: &[String], quorum_threshold: f64) -> Self {
        let n = node_ids.len();
        // Each node needs ceil(n * quorum_threshold) - 1 peers to agree
        // (minus 1 because the node itself is not in its own trusted list)
        let threshold = ((n as f64 * quorum_threshold).ceil() as usize).saturating_sub(1).max(1);

        let mut quorum_slices = HashMap::new();

        for node_id in node_ids {
            let trusted: Vec<String> = node_ids
                .iter()
                .filter(|id| *id != node_id)
                .cloned()
                .collect();

            quorum_slices.insert(
                node_id.clone(),
                QuorumSlice {
                    node_id:       node_id.clone(),
                    trusted_nodes: trusted,
                    threshold,
                },
            );
        }

        Self {
            nodes:            node_ids.to_vec(),
            quorum_slices,
            quorum_threshold,
        }
    }


    /// Find all pairs of nodes whose outputs are semantically similar
    /// Returns: Vec of (node_a, node_b, similarity)
    fn find_agreeing_pairs<'a>(
        &self,
        nodes: &'a [FbaNode],
        similarity_threshold: f64,
    ) -> Vec<(&'a FbaNode, &'a FbaNode, f64)> {
        let mut pairs = Vec::new();
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let sim = compute_code_similarity(&nodes[i].rust_code, &nodes[j].rust_code);
                if sim >= similarity_threshold {
                    pairs.push((&nodes[i], &nodes[j], sim));
                }
            }
        }
        pairs
    }

    /// SCP Quorum Intersection check:
    /// A statement is ratified when every node's quorum slice is satisfied.
    /// For 3-node network: need at least 2 nodes agreeing so that each of those
    /// nodes sees at least 1 trusted peer agreeing — satisfying their quorum slice.
    fn check_quorum_intersection(
        &self,
        agreeing_node_ids: &[&str],
    ) -> bool {
        if agreeing_node_ids.len() < 2 {
            return false;
        }

        // Every agreeing node must have its quorum slice satisfied
        let all_satisfied = agreeing_node_ids.iter().all(|node_id| {
            if let Some(slice) = self.quorum_slices.get(*node_id) {
                let satisfied = slice.is_satisfied(agreeing_node_ids);
                info!(
                    "QuorumSlice[{}]: threshold={} trusted={:?} agreeing={:?} → satisfied={}",
                    node_id, slice.threshold, slice.trusted_nodes, agreeing_node_ids, satisfied
                );
                satisfied
            } else {
                warn!("No QuorumSlice found for node: {}", node_id);
                false
            }
        });

        all_satisfied
    }

    /// Check consensus across nodes using FBA quorum intersection
    /// Returns FbaResult with status and winning Rust code
    pub fn check_consensus(
        &self,
        nodes: &[FbaNode],
        similarity_threshold: f64,
        confidence_threshold: f64,
        bayesian: &crate::bayesian::BayesianResult,
    ) -> FbaResult {
        info!(
            "FBA check_consensus: {} nodes, sim_threshold={:.2}, conf_threshold={:.2}",
            nodes.len(), similarity_threshold, confidence_threshold
        );

        let k_star = bayesian.k_star;

        // Need at least 2 nodes for quorum (2/3)
        if nodes.len() < 2 {
            warn!("Quorum violation: only {} node(s) available", nodes.len());
            return FbaResult {
                status: "QUORUM_VIOLATION".to_string(),
                rust_code: None,
                confidence: 0.0,
                semantic_similarity: 0.0,
                bayesian_guarantee: "VIOLATED".to_string(),
                martingale_satisfied: false,
                k_star,
                node_results: nodes.to_vec(),
                paper_reference: "arxiv:2507.11768".to_string(),
            };
        }

        // Step 1: Find best agreeing pair by similarity
        let mut best_similarity = 0.0f64;
        let mut best_pair: Option<(&FbaNode, &FbaNode)> = None;

        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let sim = compute_code_similarity(&nodes[i].rust_code, &nodes[j].rust_code);
                if sim > best_similarity {
                    best_similarity = sim;
                    best_pair = Some((&nodes[i], &nodes[j]));
                }
            }
        }

        info!("Best pair similarity: {:.3}", best_similarity);

        // Step 2: Check confidence — at least 2 nodes must meet threshold
        let confident_nodes: Vec<&FbaNode> = nodes
            .iter()
            .filter(|n| n.confidence >= confidence_threshold)
            .collect();

        let all_confident = confident_nodes.len() >= 2;
        if !all_confident {
            warn!(
                "Only {}/{} nodes meet confidence threshold {:.2}",
                confident_nodes.len(), nodes.len(), confidence_threshold
            );
        }

        // Step 3: SCP Quorum Intersection — wire QuorumSlice into consensus decision
        // Find agreeing pairs (similarity >= threshold)
        let agreeing_pairs = self.find_agreeing_pairs(nodes, similarity_threshold);

        // Collect all node IDs that are part of at least one agreeing pair
        let mut agreeing_ids: Vec<&str> = Vec::new();
        for (a, b, sim) in &agreeing_pairs {
            info!("Agreeing pair: {} ↔ {} (sim={:.3})", a.node_id, b.node_id, sim);
            if !agreeing_ids.contains(&a.node_id.as_str()) {
                agreeing_ids.push(a.node_id.as_str());
            }
            if !agreeing_ids.contains(&b.node_id.as_str()) {
                agreeing_ids.push(b.node_id.as_str());
            }
        }

        // Step 4: Verify quorum intersection per SCP protocol
        let quorum_intersection = self.check_quorum_intersection(&agreeing_ids);

        info!(
            "Quorum intersection: {} | agreeing_nodes={:?}",
            quorum_intersection, agreeing_ids
        );

        // Step 5: Martingale satisfied = quorum intersection + confidence + similarity
        let martingale_satisfied = best_similarity >= similarity_threshold
            && all_confident
            && quorum_intersection;

        let status = if martingale_satisfied {
            "CONSENSUS_REACHED".to_string()
        } else if nodes.len() >= 2 {
            "DISAGREEMENT".to_string()
        } else {
            "QUORUM_VIOLATION".to_string()
        };

        // Pick winning code — highest confidence node from best pair
        let rust_code = if martingale_satisfied {
            best_pair.map(|(a, b)| {
                if a.confidence >= b.confidence {
                    a.rust_code.clone()
                } else {
                    b.rust_code.clone()
                }
            })
        } else {
            None
        };

        // Combined confidence (geometric mean)
        let combined_confidence = nodes
            .iter()
            .map(|n| n.confidence)
            .product::<f64>()
            .powf(1.0 / nodes.len() as f64);

        let bayesian_guarantee = if martingale_satisfied {
            "IN_REALIZATION".to_string()
        } else {
            "IN_EXPECTATION_ONLY".to_string()
        };

        info!(
            "FBA result: {} | confidence={:.3} | similarity={:.3} | \
             quorum_intersection={} | bayesian={}",
            status, combined_confidence, best_similarity,
            quorum_intersection, bayesian_guarantee
        );

        FbaResult {
            status,
            rust_code,
            confidence: combined_confidence,
            semantic_similarity: best_similarity,
            bayesian_guarantee,
            martingale_satisfied,
            k_star,
            node_results: nodes.to_vec(),
            paper_reference: "arxiv:2507.11768".to_string(),
        }
    }
}

/// FbaResult — 3-node pipeline result type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FbaResult {
    pub status: String,
    pub rust_code: Option<String>,
    pub confidence: f64,
    pub semantic_similarity: f64,
    pub bayesian_guarantee: String,
    pub martingale_satisfied: bool,
    pub k_star: usize,
    pub node_results: Vec<FbaNode>,
    pub paper_reference: String,
}

/// Semantic Code Equivalence Engine — 5-Layer Analysis
pub fn compute_code_similarity(code_a: &str, code_b: &str) -> f64 {
    if code_a.is_empty() && code_b.is_empty() { return 1.0; }
    if code_a.is_empty() || code_b.is_empty() { return 0.0; }

    let struct_a = structural_fingerprint(code_a);
    let struct_b = structural_fingerprint(code_b);
    let layer1 = strsim::jaro_winkler(&struct_a, &struct_b);

    let nums_a = extract_numeric_literals(code_a);
    let nums_b = extract_numeric_literals(code_b);
    let layer2 = compare_numeric_sets(&nums_a, &nums_b);

    let types_a = extract_rust_types(code_a);
    let types_b = extract_rust_types(code_b);
    let layer3 = jaccard_similarity(&types_a, &types_b);

    let ops_a = extract_operator_sequence(code_a);
    let ops_b = extract_operator_sequence(code_b);
    let layer4 = strsim::jaro(&ops_a, &ops_b);

    let kw_a = keyword_density_vector(code_a);
    let kw_b = keyword_density_vector(code_b);
    let layer5 = cosine_similarity(&kw_a, &kw_b);

    let similarity = 0.20 * layer1
        + 0.15 * layer2
        + 0.30 * layer3
        + 0.15 * layer4
        + 0.20 * layer5;

    info!(
        "Similarity layers: struct={:.3} nums={:.3} types={:.3} ops={:.3} kw={:.3} → final={:.3}",
        layer1, layer2, layer3, layer4, layer5, similarity
    );

    similarity.clamp(0.0, 1.0)
}

fn structural_fingerprint(code: &str) -> String {
    let mut result = String::new();
    let keywords = [
        "fn", "let", "mut", "pub", "struct", "impl", "if", "else",
        "for", "while", "return", "use", "mod", "const", "static",
        "match", "loop", "break", "continue", "self", "Self",
        "true", "false", "where", "async", "await", "move",
        "f64", "f32", "i64", "i32", "i16", "i8", "u64", "u32",
        "u16", "u8", "usize", "isize", "bool", "String", "str",
        "Vec", "Option", "Result", "Some", "None", "Ok", "Err",
    ];

    let mut word = String::new();

    for c in code.chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            if !word.is_empty() {
                if word.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    result.push_str("NUM");
                } else if keywords.contains(&word.as_str()) {
                    result.push_str(&word);
                } else {
                    result.push('_');
                }
                word.clear();
            }
            if !c.is_whitespace() {
                result.push(c);
            } else {
                result.push(' ');
            }
        }
    }
    if !word.is_empty() {
        if word.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            result.push_str("NUM");
        } else if keywords.contains(&word.as_str()) {
            result.push_str(&word);
        } else {
            result.push('_');
        }
    }

    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_numeric_literals(code: &str) -> Vec<String> {
    let mut nums = Vec::new();
    let mut current = String::new();
    let mut in_num = false;

    for c in code.chars() {
        if c.is_ascii_digit() || (c == '.' && in_num) {
            current.push(c);
            in_num = true;
        } else if c == '-' && !in_num {
            current.push(c);
        } else {
            if in_num && !current.is_empty() && current != "-" {
                nums.push(normalize_number(&current));
            }
            current.clear();
            in_num = false;
        }
    }
    if in_num && !current.is_empty() {
        nums.push(normalize_number(&current));
    }

    nums.sort();
    nums.dedup();
    nums
}

fn normalize_number(s: &str) -> String {
    if let Ok(f) = s.parse::<f64>() {
        if f.fract() == 0.0 {
            return format!("{}", f as i64);
        }
        let formatted = format!("{:.10}", f);
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        return trimmed.to_string();
    }
    s.to_string()
}

fn compare_numeric_sets(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() { return 1.0; }
    if a.is_empty() || b.is_empty() { return 0.3; }

    let filter_constants = |nums: &[String]| -> std::collections::HashSet<String> {
        nums.iter()
            .filter(|n| {
                if let Ok(f) = n.parse::<f64>() {
                    f.abs() <= 1000.0 && f.abs() > 0.0
                } else { false }
            })
            .cloned()
            .collect()
    };

    let set_a = filter_constants(a);
    let set_b = filter_constants(b);

    if set_a.is_empty() && set_b.is_empty() { return 0.8; }

    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();

    if union == 0 { 1.0 } else {
        let jaccard = intersection as f64 / union as f64;
        if intersection == set_a.len() && intersection == set_b.len() { 1.0 } else { jaccard }
    }
}

fn extract_rust_types(code: &str) -> Vec<String> {
    let type_keywords = [
        "f64", "f32", "i64", "i32", "i16", "i8",
        "u64", "u32", "u16", "u8", "usize", "isize",
        "bool", "String", "str", "Vec", "Option",
        "Result", "HashMap", "HashSet",
    ];

    let mut found = Vec::new();
    for token in code.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if type_keywords.contains(&token) {
            found.push(token.to_string());
        }
    }
    found.sort();
    found.dedup();
    found
}

fn jaccard_similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() { return 1.0; }
    let set_a: std::collections::HashSet<&String> = a.iter().collect();
    let set_b: std::collections::HashSet<&String> = b.iter().collect();
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 { 1.0 } else { intersection as f64 / union as f64 }
}

fn extract_operator_sequence(code: &str) -> String {
    code.chars()
        .filter(|c| matches!(c, '*' | '/' | '+' | '-' | '%' | '=' | '!' | '<' | '>'))
        .collect()
}

const RUST_KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "pub", "struct", "impl",
    "if", "else", "for", "while", "return", "match",
    "use", "mod", "const", "async", "await",
];

fn keyword_density_vector(code: &str) -> Vec<f64> {
    let total_words = code.split_whitespace().count().max(1) as f64;
    RUST_KEYWORDS
        .iter()
        .map(|kw| {
            let count = code.split_whitespace()
                .filter(|w| {
                    let clean = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
                    clean == *kw
                })
                .count();
            count as f64 / total_words
        })
        .collect()
}

fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() { return 0.0; }
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let mag_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if mag_a == 0.0 && mag_b == 0.0 { return 1.0; }
    if mag_a == 0.0 || mag_b == 0.0 { return 0.5; }
    (dot / (mag_a * mag_b)).clamp(0.0, 1.0)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bayesian() -> crate::bayesian::BayesianResult {
        crate::bayesian::compute_k_star(&crate::bayesian::BayesianParams {
            cobol_line_count: 50,
            epsilon: 0.01,
            theta: 2.5,
        })
    }

    fn make_node(id: &str, name: &str, code: &str, confidence: f64) -> FbaNode {
        FbaNode {
            node_id: id.to_string(),
            model_name: name.to_string(),
            rust_code: code.to_string(),
            confidence,
            cot_steps_used: 46,
            raw_response: "".to_string(),
        }
    }

    #[test]
    fn test_quorum_slice_satisfied() {
        let slice = QuorumSlice {
            node_id: "claude_opus_4_6".to_string(),
            trusted_nodes: vec![
                "deepseek_v3_nebius".to_string(),
                "llama_3_3_70b".to_string(),
            ],
            threshold: 1,
        };
        // 1 trusted node agrees → satisfied
        assert!(slice.is_satisfied(&["deepseek_v3_nebius"]));
        // Both trusted nodes agree → satisfied
        assert!(slice.is_satisfied(&["deepseek_v3_nebius", "llama_3_3_70b"]));
        // No trusted node agrees → not satisfied
        assert!(!slice.is_satisfied(&["unknown_node"]));
        // Empty → not satisfied
        assert!(!slice.is_satisfied(&[]));
    }

    #[test]
    fn test_quorum_intersection_two_nodes() {
        let net = FbaNetwork::new_three_node();
        // Claude + DeepSeek agree → each sees the other as trusted → intersection holds
        assert!(net.check_quorum_intersection(&["claude_opus_4_6", "deepseek_v3_nebius"]));
        // All 3 agree → intersection holds
        assert!(net.check_quorum_intersection(&[
            "claude_opus_4_6", "deepseek_v3_nebius", "llama_3_3_70b"
        ]));
        // Only 1 node → no intersection
        assert!(!net.check_quorum_intersection(&["claude_opus_4_6"]));
        // Empty → no intersection
        assert!(!net.check_quorum_intersection(&[]));
    }

    #[test]
    fn test_three_node_network_structure() {
        let net = FbaNetwork::new_three_node();
        assert_eq!(net.nodes.len(), 3);
        assert!(net.quorum_slices.contains_key("claude_opus_4_6"));
        assert!(net.quorum_slices.contains_key("deepseek_v3_nebius"));
        assert!(net.quorum_slices.contains_key("llama_3_3_70b"));
        assert_eq!(net.quorum_threshold, 0.667);
    }

    #[test]
    fn test_three_node_consensus_all_agree() {
        let network = FbaNetwork::new_three_node();
        let code = "fn calculate(x: f64) -> f64 { x * 0.055 }";
        let nodes = vec![
            make_node("claude_opus_4_6", "Claude Opus 4.6", code, 0.94),
            make_node("deepseek_v3_nebius", "DeepSeek V3.2 (Nebius)", code, 0.91),
            make_node("llama_3_3_70b", "Llama-3.3-70B (Nebius)", code, 0.90),
        ];
        let result = network.check_consensus(&nodes, 0.75, 0.85, &make_bayesian());
        assert_eq!(result.status, "CONSENSUS_REACHED");
        assert!(result.rust_code.is_some());
        assert_eq!(result.bayesian_guarantee, "IN_REALIZATION");
        assert!(result.martingale_satisfied);
    }

    #[test]
    fn test_three_node_quorum_with_one_failure() {
        let network = FbaNetwork::new_three_node();
        let code = "fn calculate(x: f64) -> f64 { x * 0.055 }";
        let nodes = vec![
            make_node("claude_opus_4_6", "Claude Opus 4.6", code, 0.94),
            make_node("deepseek_v3_nebius", "DeepSeek V3.2 (Nebius)", code, 0.91),
        ];
        let result = network.check_consensus(&nodes, 0.75, 0.85, &make_bayesian());
        assert_eq!(result.status, "CONSENSUS_REACHED");
        assert!(result.rust_code.is_some());
        assert_eq!(result.bayesian_guarantee, "IN_REALIZATION");
    }

    #[test]
    fn test_quorum_violation_one_node() {
        let network = FbaNetwork::new_three_node();
        let nodes = vec![
            make_node("claude_opus_4_6", "Claude Opus 4.6",
                "fn calculate(x: f64) -> f64 { x * 0.055 }", 0.94),
        ];
        let result = network.check_consensus(&nodes, 0.75, 0.85, &make_bayesian());
        assert_eq!(result.status, "QUORUM_VIOLATION");
        assert!(result.rust_code.is_none());
    }

    #[test]
    fn test_three_node_disagreement() {
        let network = FbaNetwork::new_three_node();
        let nodes = vec![
            make_node("claude_opus_4_6", "Claude Opus 4.6",
                "fn interest(p: f64, r: f64) -> f64 { p * r / 100.0 }", 0.94),
            make_node("deepseek_v3_nebius", "DeepSeek V3.2",
                "fn process_payroll(hours: f64, rate: f64) -> HashMap<String, f64> { HashMap::new() }", 0.91),
            make_node("llama_3_3_70b", "Llama-3.3-70B",
                "struct Employee { name: String, salary: f64 } impl Employee { fn new() -> Self { todo!() } }", 0.90),
        ];
        let result = network.check_consensus(&nodes, 0.75, 0.85, &make_bayesian());
        assert_eq!(result.status, "DISAGREEMENT");
        assert!(result.rust_code.is_none());
    }

    #[test]
    fn test_code_similarity_identical() {
        let code = "fn foo() -> i32 { 42 }";
        assert!((compute_code_similarity(code, code) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_semantic_equivalence_different_names() {
        let claude = r#"
            pub fn calculate_interest(ws_principal: f64, ws_rate: f64) -> f64 {
                let ws_interest = ws_principal * ws_rate / 100.0;
                println!("CALCULATED INTEREST: {:.2}", ws_interest);
                ws_interest
            }
        "#;
        let deepseek_v3 = r#"
            pub fn compute_interest(principal: f64, rate: f64) -> f64 {
                let interest = principal * rate / 100.0;
                println!("CALCULATED INTEREST: {:.2}", interest);
                interest
            }
        "#;
        let sim = compute_code_similarity(claude, deepseek_v3);
        assert!(sim > 0.75, "Expected > 0.75, got {:.3}", sim);
    }

    #[test]
    fn test_genuinely_different_logic() {
        let a = "fn interest(p: f64, r: f64) -> f64 { p * r / 100.0 }";
        let b = r#"
            fn process_payroll(employees: Vec<Employee>) -> HashMap<String, f64> {
                let mut result = HashMap::new();
                for emp in employees {
                    result.insert(emp.id.clone(), emp.hours * emp.rate);
                }
                result
            }
        "#;
        let sim = compute_code_similarity(a, b);
        assert!(sim < 0.6, "Expected < 0.6, got {:.3}", sim);
    }
}