# AgentX-Phase2 — Mainframe Modernization Pipeline
### FBA 49-Node Consensus | Zero-Trust Security | Kubernetes + Istio + Kiali
**AgentX - AgentBeats Competition | Phase 2 Sprint 1 | Business Process Agent Track**

[![CI](https://github.com/tenalirama2005/AgentX-Phase2/actions/workflows/agentx-deploy.yml/badge.svg)](https://github.com/tenalirama2005/AgentX-Phase2/actions/workflows/agentx-deploy.yml)
[![Cilium](https://img.shields.io/badge/Cilium-Isovalent-orange)](https://www.credly.com/badges/40ec4b44-6e12-4aba-9512-31303e4d0733)
[![Solo.io](https://img.shields.io/badge/Solo.io-Velocity-blue)](https://www.credly.com/earner/earned/badge/dd16b89b-f8b8-4dae-9800-2bf3429b1a3e)
[![Kubernetes](https://img.shields.io/badge/Kubernetes-v1.35-blue)](https://kubernetes.io)
[![Istio](https://img.shields.io/badge/Istio-v1.29.1-purple)](https://istio.io)
[![Rust](https://img.shields.io/badge/Rust-1.94-orange)](https://rustlang.org)
[![Paper](https://img.shields.io/badge/Paper-arxiv%3A2507.11768-red)](https://arxiv.org/abs/2507.11768)

---

## Demo Video

[![AgentX-Phase2 Demo](https://img.youtube.com/vi/fXXyVwlup0Y/maxresdefault.jpg)](https://youtu.be/fXXyVwlup0Y)

▶️ [Watch on YouTube](https://youtu.be/fXXyVwlup0Y) — 6:35 minutes



## What is AgentX-Phase2?

AgentX-Phase2 is a **Kubernetes-native multi-agent pipeline** that modernizes 
legacy COBOL mainframe code into production-ready Rust using **49 AI models 
voting in parallel** with Federated Byzantine Agreement (FBA) consensus.
```
COBOL Source → S3 → Green Agent → Purple Agent (49 FBA nodes) → Rust Output → S3
```

Every MCP call is enforced through **AgentGateway** with JWT + RBAC zero-trust 
security, backed by **Istio service mesh** with **Kiali** visual proof.

---

## Architecture
```
┌─────────────────────────────────────────────────────────┐
│           mainframe-modernization namespace              │
│                                                         │
│  green_agent ──→ AgentGateway ──→ s3_mcp               │
│      │               │              │                   │
│      └──→ purple_agent    cobol_mcp rust_mcp ai_mcp    │
│              │                                          │
│         49 FBA nodes                                    │
│    (Nebius + Anthropic Claude)                          │
└─────────────────────────────────────────────────────────┘
         │
    Istio Service Mesh (mTLS + AuthorizationPolicy)
         │
    Kiali Dashboard (visual proof)
```

### Agents
| Agent | Role | Technology |
|-------|------|------------|
| 🟢 **green_agent** | Orchestrator — sets tasks, evaluates results | Rust + Actix-web |
| 🟣 **purple_agent** | Participant — 49-node FBA consensus engine | Rust + Actix-web |

### MCP Servers
| Server | Purpose | Port |
|--------|---------|------|
| **agent_gateway** | JWT issuance + RBAC enforcement | 8090 |
| **s3_mcp** | AWS S3 fetch/save with GatewayAuth middleware | 8081 |
| **cobol_mcp** | GnuCOBOL compiler | 8083 |
| **rust_mcp** | Cargo compiler + validation | 8084 |
| **ai_mcp** | Claude + Nebius AI models | 8082 |

---

## Quick Start

### Prerequisites
- Docker Desktop running
- kind CLI installed
- kubectl installed
- `.env` file with API keys (see `.env.example`)

### Deploy
```bash
# Clone the repo
git clone https://github.com/tenalirama2005/AgentX-Phase2
cd AgentX-Phase2

## Before Every Run — Verify Credentials

Before running `./deploy.sh --run-pipeline`, always verify your credentials 
are correctly injected into the cluster:
```bash
# Verify AWS credentials in s3-mcp pod
kubectl exec -n mainframe-modernization \
  $(kubectl get pod -n mainframe-modernization -l app=s3-mcp -o jsonpath='{.items[0].metadata.name}') \
  -c s3-mcp -- env | grep -E "AWS_ACCESS_KEY_ID|AWS_SECRET|AWS_REGION"

# Verify Nebius + Anthropic credentials in purple-agent pod  
kubectl exec -n mainframe-modernization \
  $(kubectl get pod -n mainframe-modernization -l app=purple-agent -o jsonpath='{.items[0].metadata.name}') \
  -c purple-agent -- env | grep -E "NEBIUS|ANTHROPIC|CLAUDE"
```

If any values are empty, re-inject secrets:
```bash
ANTH=$(grep "^ANTHROPIC_API_KEY" .env | cut -d'=' -f2- | tr -d '\r\n')
AKID=$(grep "^AWS_ACCESS_KEY_ID" .env | cut -d'=' -f2- | tr -d '\r\n')
ASEC=$(grep "^AWS_SECRET_ACCESS_KEY" .env | cut -d'=' -f2- | tr -d '\r\n')
NEBIUS=$(grep "^NEBIUS_API_KEY" .env | cut -d'=' -f2- | tr -d '\r\n')
PURP_KEY=$(grep "^PURPLE_AGENT_API_KEY" .env | cut -d'=' -f2- | tr -d '\r\n')
GREEN_KEY=$(grep "^GREEN_AGENT_API_KEY" .env | cut -d'=' -f2- | tr -d '\r\n')

echo "AWS    : ${AKID:0:8}..."
echo "Nebius : ${NEBIUS:0:10}..."
echo "Anthropic: ${ANTH:0:10}..."

kubectl delete secret s3-mcp-credentials purple-agent-credentials \
  green-agent-credentials ai-mcp-credentials -n mainframe-modernization

kubectl create secret generic s3-mcp-credentials \
  -n mainframe-modernization \
  --from-literal=aws-access-key-id="$AKID" \
  --from-literal=aws-secret-access-key="$ASEC" \
  --from-literal=aws-region="us-east-1"

kubectl create secret generic purple-agent-credentials \
  -n mainframe-modernization \
  --from-literal=api-key="$PURP_KEY" \
  --from-literal=anthropic-api-key="$ANTH" \
  --from-literal=nebius-api-key="$NEBIUS"

kubectl create secret generic green-agent-credentials \
  -n mainframe-modernization \
  --from-literal=api-key="$GREEN_KEY" \
  --from-literal=aws-access-key-id="$AKID" \
  --from-literal=aws-secret-access-key="$ASEC" \
  --from-literal=aws-region="us-east-1"

kubectl create secret generic ai-mcp-credentials \
  -n mainframe-modernization \
  --from-literal=claude-api-key="$ANTH" \
  --from-literal=nebius-api-key="$NEBIUS"

kubectl rollout restart deployment -n mainframe-modernization
sleep 90
```
> **Note:** The GitHub Actions CI pipeline runs infrastructure tests 
> (pod deployment, security proof). The full FBA pipeline with 49 AI 
> models is best run locally due to GitHub Actions runner limitations.
> Run `./deploy.sh --run-pipeline` locally for full results.

# Create kind cluster + deploy everything
./deploy.sh

# Run COBOL → Rust pipeline with 49-node FBA consensus
./deploy.sh --run-pipeline

# Prove zero-trust security
./deploy.sh --test-security

# Open Kiali service mesh dashboard
./deploy.sh --ui

# Check pod status
./deploy.sh --status

# Sleep/wake cluster
./deploy.sh --sleep
./deploy.sh --wake

# Teardown
./deploy.sh --teardown
```

---

## Zero-Trust Security Proof
```bash
./deploy.sh --test-security
```
```
[TEST 1] purple_agent → s3_mcp DIRECT          BLOCKED
         Direct access denied — GatewayAuth middleware (Rust)

[TEST 2] purple_agent acquires JWT token        ISSUED
         Token: ****eXzc (masked for security)

[TEST 3] purple_agent → gateway → s3_mcp       BLOCKED (wrong role)
         HTTP 403 — AgentGateway RBAC denied

[TEST 4] green_agent → gateway → s3_mcp        ALLOWED (correct role)
         HTTP 200 — authorized access
```

### Security Layers
1. **GatewayAuth Middleware** (Rust) — s3_mcp rejects any request without 
   `X-AgentGateway-Token` header, regardless of network path
2. **JWT + RBAC** — AgentGateway enforces role-based access on every MCP call
3. **Istio AuthorizationPolicy** — service mesh layer blocks unauthorized traffic
4. **Kubernetes NetworkPolicy** — default-deny-all with explicit allow rules

> **Zero-trust principle:** A valid JWT is not enough — role must match operation.
> Test 3 proves it: purple_agent has a valid token and is still blocked.

---

## FBA Pipeline Results
```bash
./deploy.sh --run-pipeline
```
### Sample Output
```
 Model                          Conf  Bar (25 chars = 100%)     Status
  ====================================================================
  [01] glm_4_7_fp8                98%  [########################.] OK
  [02] deepseek_r1_0528           98%  [########################.] OK
  [03] llama_3_1_8b_fast          95%  [#######################..] OK
  [04] qwen2_5_coder_7b           95%  [#######################..] OK
  [05] deepseek_v3_2              95%  [#######################..] OK
  [06] gemma3_27b                 95%  [#######################..] OK
  [07] glm_4_5                    95%  [#######################..] OK
  [08] nemotron_nano_30b          95%  [#######################..] OK
  [09] deepseek_v3_0324           95%  [#######################..] OK
  [10] qwen3_30b                  95%  [#######################..] OK
  ...
  [28] nemotron_nano_12b          85%  [#####################....] OK
  ====================================================================

  Status    : CONSENSUS_REACHED
  Confidence: 0.9327429562161073
  Similarity: 0.9979196217494092
  k*        : 89 reasoning steps per model (Bayesian minimum)
              k* = ceil(θ × √n × log(1/ε)) where n = COBOL line count
              Fixed for interest_calc.cbl — changes only if COBOL input changes
  Guarantee : IN_REALIZATION
  Paper     : arxiv:2507.11768
  Download  : (pre-signed URL valid 1 hour)
  FBA Report: (pre-signed URL valid 1 hour)
  Per-node  : modernized/review/<uuid>/<model_name>/interest_calc.rs
```

---

## Understanding Pipeline Output

### Presigned URL (Download Link)
The `Download` URL points to the **consensus winner** — the single best Rust 
translation selected by the FBA algorithm from all responding models. This 
pre-signed AWS URL is valid for 1 hour and requires no authentication.

### Per-Node Outputs
Each responding model generates its own Rust translation stored at:
```
s3://mainframe-refactor-lab-venkatnagala/<review_folder>/<model_name>/interest_calc.rs
```
Examples:
```
modernized/review/<uuid>/claude_opus_4_6/interest_calc.rs
modernized/review/<uuid>/llama_3_1_8b/interest_calc.rs
modernized/review/<uuid>/deepseek_v3_2/interest_calc.rs
```

### Understanding k*
`k* = 89` is the mathematically computed minimum number of Chain-of-Thought 
reasoning steps each model must perform, calculated as:
```
k* = ⌈θ × √n × log(1/ε)⌉
```

Where:
- `θ` = confidence threshold (0.85)
- `n` = number of COBOL source lines
- `ε` = acceptable error probability

**k* = 89 is fixed** for `interest_calc.cbl` — it changes only if a different 
COBOL file with more/fewer lines is processed.

**49 models × 89 reasoning steps = 4,361 total verified reasoning steps** 
per pipeline run.

### Bar Chart Legend
| Status | Threshold | Description |
|--------|-----------|-------------|
| [OK]   | ≥ 85% | Model in consensus |
| [WARN] | ≥ 70% | Borderline |
| [LOW]  | < 70% | Below threshold |

---

## Infrastructure

| Component | Technology |
|-----------|------------|
| Cluster | kind (Kubernetes in Docker) |
| Service Mesh | Istio 1.29.1 |
| Observability | Kiali + Prometheus |
| CNI | kindnet + Istio sidecar |
| Security | NetworkPolicy + AuthorizationPolicy + GatewayAuth |
| Cloud | AWS S3 (us-east-1) |
| AI Models | Anthropic Claude + 30 Nebius models |
| Container Registry | Docker Hub (tenalirama2026) |
| CI/CD | GitHub Actions |

---

## AI Models (49 Nodes)

**1 Anchor Model + 48 Nebius Models** voting in parallel via FBA consensus.

### Anchor Model
| Model | Provider | Tokens |
|-------|----------|--------|
| Claude Opus 4.6 | Anthropic | 8192 |

### Tier 1 — Large Frontier Models (8192 tokens)
| Model | Provider |
|-------|---------|
| DeepSeek-V3.2 | Nebius |
| DeepSeek-V3-0324 | Nebius |
| DeepSeek-R1-0528 | Nebius |
| DeepSeek-R1-0528-fast | Nebius |
| Llama-3.3-70B-Instruct | Nebius |
| Llama-3.3-70B-Instruct-fast | Nebius |
| Llama-3.1-Nemotron-Ultra-253B | Nebius |
| Qwen3-235B-A22B-Instruct | Nebius |
| Qwen3-235B-A22B-Thinking | Nebius |
| Qwen3-235B-A22B-Thinking-fast | Nebius |
| Qwen3-Coder-480B-A35B | Nebius |
| Qwen3-Next-80B-Thinking | Nebius |
| Qwen3.5-397B | Nebius |
| Hermes-4-405B | Nebius |
| Kimi-K2-Instruct | Nebius |
| Kimi-K2-Thinking | Nebius |
| Kimi-K2.5 | Nebius |
| MiniMax-M2.1 | Nebius |
| MiniMax-M2.5 | Nebius |
| GLM-4.5 | Nebius |
| GLM-5 | Nebius |
| GPT-OSS-120B | Nebius |
| GPT-OSS-120B-fast | Nebius |
| Nemotron-3-Super-120B | Nebius |

### Tier 2 — Medium Models (4096 tokens)
| Model | Provider |
|-------|---------|
| DeepSeek-V3.2-fast | Nebius |
| DeepSeek-V3-0324-fast | Nebius |
| Llama-3.3-70B-Instruct (alt) | Nebius |
| Qwen3-32B | Nebius |
| Qwen3-32B-fast | Nebius |
| Qwen3-30B-A3B-Instruct | Nebius |
| Qwen3-Coder-30B-A3B | Nebius |
| Qwen3-Next-80B-Thinking-fast | Nebius |
| Qwen3.5-397B-fast | Nebius |
| Hermes-4-70B | Nebius |
| INTELLECT-3-106B | Nebius |
| Kimi-K2.5-fast | Nebius |
| GLM-4.5-Air | Nebius |
| GLM-4.7-FP8 | Nebius |
| Gemma-3-27B-IT | Nebius |
| Gemma-3-27B-IT-fast | Nebius |
| NVIDIA-Nemotron-Nano-30B | Nebius |
| GPT-OSS-20B | Nebius |

### Tier 3 — Fast Models (2048 tokens)
| Model | Provider |
|-------|---------|
| Meta-Llama-3.1-8B-Instruct | Nebius |
| Meta-Llama-3.1-8B-Instruct-fast | Nebius |
| Nemotron-Nano-V2-12B | Nebius |
| Qwen2.5-Coder-7B | Nebius |
| Gemma-2-9B-IT-fast | Nebius |
| Gemma-2-2B-IT | Nebius |

### Summary
| Tier | Count | Description |
|------|-------|-------------|
| Anchor | 1 | Claude Opus 4.6 (Anthropic) |
| Tier 1 | 24 | Large frontier models ≥70B |
| Tier 2 | 18 | Medium models 20B-70B |
| Tier 3 | 6 | Fast small models <20B |
| **Total** | **49** | **1 Anthropic + 48 Nebius** |

---

## Badges

- 🏅 [Cilium Isovalent Certified](https://www.credly.com/badges/40ec4b44-6e12-4aba-9512-31303e4d0733) — Sep 2024
- 🏅 [Solo.io Velocity Certified](https://www.credly.com/earner/earned/badge/dd16b89b-f8b8-4dae-9800-2bf3429b1a3e)

---

## GitHub Actions CI/CD

### What CI Tests Run
| Test | Description | Status |
|------|-------------|--------|
| Deploy | kind cluster + Istio + all pods 2/2 | ✅ |
| Security | 4 zero-trust JWT+RBAC tests | ✅ |
| Pipeline | 49-model FBA consensus | ✅ |

### Trigger Pipeline Manually
```bash
# Install gh CLI
sudo apt install gh -y
gh auth login

# Trigger deployment pipeline manually
gh workflow run agentx-deploy.yml \
  --repo tenalirama2005/AgentX-Phase2 \
  --field run_pipeline=true \
  --field run_security=true

# Watch live
gh run watch --repo tenalirama2005/AgentX-Phase2
```

> **Note:** Pipeline also triggers automatically on every `git push` to main branch.
> GitHub Pro runners are used — no self-hosted runner required.
```

---

## Paper Reference

This pipeline implements the FBA consensus algorithm described in:

**arxiv:2507.11768** — Federated Byzantine Agreement for AI Model Consensus

The Bayesian guarantee `IN_REALIZATION` means the mathematical proof of 
correctness is actively being verified against the FBA theorem.

---

## Author

**Venkateshwar Rao Nagala** | For the Cloud By the Cloud
- GitHub: [@tenalirama2005](https://github.com/tenalirama2005)
- Docker Hub: [tenalirama2026](https://hub.docker.com/u/tenalirama2026)

---

*AgentX - AgentBeats Competition | Phase 2 Sprint 1 | March 2026*
*UC Berkeley RDI | Business Process Agent Track*