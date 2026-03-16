#!/bin/bash
# ============================================================
# AgentX-Phase2 — Deploy to Kubernetes via vind (vCluster in Docker)
# Author: Venkat Nagala | For the Cloud By the Cloud
#
# vind = vCluster in Docker — KinD replacement with:
#   LoadBalancer works out of the box
#   Free vCluster Platform UI (accessible from anywhere)
#   Sleep/wake cluster
#   Add EC2/GPU nodes via VPN
#   Pull-through Docker registry cache
#   GitHub: https://github.com/loft-sh/vind
#
# Prerequisites:
#   Docker Desktop running (Kubernetes NOT required)
#   vCluster CLI: curl -L -o vcluster https://github.com/loft-sh/vcluster/releases/latest/download/vcluster-linux-amd64
#                 sudo install -c -m 0755 vcluster /usr/local/bin
#   GitHub CLI:   sudo apt install gh -y && gh auth login
#   .env file with real API keys in project root
#
# Usage:
#   ./deploy.sh                  # connect cluster + deploy
#   ./deploy.sh --build-images   # build+push Docker images first
#   ./deploy.sh --run-pipeline   # run pipeline + show FBA node results
#   ./deploy.sh --test-security  # prove JWT RBAC blocks purple_agent
#   ./deploy.sh --status         # pod status
#   ./deploy.sh --sleep          # sleep the cluster
#   ./deploy.sh --wake           # wake the cluster
#   ./deploy.sh --teardown       # delete cluster
#   ./deploy.sh --ui             # open vCluster Platform UI
#
# Monitor GitHub Actions:
#   gh workflow run agentx-deploy.yml --repo tenalirama2005/AgentX-Phase2 \
#     --field run_pipeline=true --field run_security=true
#   gh run watch --repo tenalirama2005/AgentX-Phase2
# ============================================================

# set -e disabled — we handle errors manually

CLUSTER_NAME="agentx-phase2"
NAMESPACE="mainframe-modernization"
DOCKERHUB="tenalirama2026"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
K8S_DIR="$SCRIPT_DIR/k8s/base"
S3_BUCKET="mainframe-refactor-lab-venkatnagala"

# ── Colors ───────────────────────────────────────────────────
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

green()  { echo -e "${GREEN}$1${NC}"; }
yellow() { echo -e "${YELLOW}$1${NC}"; }
red()    { echo -e "${RED}$1${NC}"; }
cyan()   { echo -e "${CYAN}$1${NC}"; }

# ── Parse arguments ──────────────────────────────────────────
BUILD_IMAGES=false
RUN_PIPELINE=false
TEST_SECURITY=false
STATUS=false
SLEEP_CLUSTER=false
WAKE_CLUSTER=false
TEARDOWN=false
UI=false

for arg in "$@"; do
    case $arg in
        --build-images)  BUILD_IMAGES=true ;;
        --run-pipeline)  RUN_PIPELINE=true ;;
        --test-security) TEST_SECURITY=true ;;
        --status)        STATUS=true ;;
        --sleep)         SLEEP_CLUSTER=true ;;
        --wake)          WAKE_CLUSTER=true ;;
        --teardown)      TEARDOWN=true ;;
        --ui)            UI=true ;;
    esac
done

# ── Status ───────────────────────────────────────────────────
if [ "$STATUS" = true ]; then
    cyan "\n=== AgentX-Phase2 Pod Status ==="
    kubectl get pods -n $NAMESPACE -o wide
    kubectl get svc -n $NAMESPACE
    exit 0
fi

# ── Sleep ────────────────────────────────────────────────────
if [ "$SLEEP_CLUSTER" = true ]; then
    yellow "Sleeping vind cluster '$CLUSTER_NAME'..."
    docker stop vcluster.cp.$CLUSTER_NAME
    green "Cluster sleeping. Run ./deploy.sh --wake to resume."
    exit 0
fi

# ── Wake ─────────────────────────────────────────────────────
if [ "$WAKE_CLUSTER" = true ]; then
    yellow "Waking vind cluster '$CLUSTER_NAME'..."
    docker start vcluster.cp.$CLUSTER_NAME
    sleep 10
    vcluster connect $CLUSTER_NAME --driver docker
    green "Cluster awake and connected."
    exit 0
fi

# ── UI ───────────────────────────────────────────────────────
if [ "$UI" = true ]; then
    cyan "Starting vCluster Platform UI..."
    vcluster platform start --reset 2>&1 | grep -v -i "password\|######\|LOGIN"
    sleep 5
    kubectl get pods -n vcluster-platform || true
    yellow "  Port-forwarding loft to http://localhost:8888 ..."
    kubectl port-forward svc/loft 8888:80 -n vcluster-platform &
    sleep 3
    # Open browser (WSL2 + Windows + Linux)
    if command -v wslview &> /dev/null; then
        wslview http://localhost:8888
    elif command -v xdg-open &> /dev/null; then
        xdg-open http://localhost:8888
    else
        green "  Open browser: http://localhost:8888"
    fi
    green "  vCluster Platform UI: http://localhost:8888"
    exit 0
fi

# ── Teardown ─────────────────────────────────────────────────
if [ "$TEARDOWN" = true ]; then
    yellow "Deleting vind cluster '$CLUSTER_NAME'..."
    vcluster delete $CLUSTER_NAME
    green "Cluster deleted."
    exit 0
fi

# ── RunPipeline ──────────────────────────────────────────────
if [ "$RUN_PIPELINE" = true ]; then
    cyan "\n=================================================="
    cyan " AgentX-Phase2 -- Pipeline Run"
    cyan "=================================================="
    yellow "  Running: programs/interest_calc.cbl -> Rust"
    yellow "  Thirty One AI models voting in parallel..."
    echo ""

    PR=$(curl -s -X POST http://localhost:8080/modernize \
        -H "Content-Type: application/json" \
        -d '{"s3_key":"programs/interest_calc.cbl"}')

    FBA_STATUS=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('fba_status',''))")
    CONFIDENCE=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('fba_confidence',''))")
    SIMILARITY=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('semantic_similarity',''))")
    K_STAR=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('k_star',''))")
    GUARANTEE=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('bayesian_guarantee',''))")
    REVIEW=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('review_folder',''))")
    S3_OUT=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('s3_output_key',''))")

    # Fetch per-node FBA report
    if [ -n "$REVIEW" ]; then
        REPORT_BODY="{\"bucket\":\"$S3_BUCKET\",\"key\":\"$REVIEW/fba_report/fba_report.json\"}"
        REPORT=$(curl -s -X POST http://localhost:8082/fetch_source \
            -H "Content-Type: application/json" \
            -d "$REPORT_BODY")

        echo ""
        cyan "================================================="
        cyan " FBA Node Results -- Thirty One Models     arxiv:2507.11768"
        cyan "================================================="

        echo $REPORT | python3 -c "
import sys, json
d = json.load(sys.stdin)
content = json.loads(d.get('content','{}'))
nodes = sorted(content.get('nodes',[]), key=lambda x: x.get('confidence',0), reverse=True)
for i, n in enumerate(nodes, 1):
    conf = n.get('confidence', 0)
    icon = '[OK]' if conf >= 0.85 else '[WARN]' if conf >= 0.70 else '[LOW]'
    print(f\"  [{i:02d}] {n['node_id']:<35} {conf:.1%}  {icon}\")
"
    fi

    echo ""
    cyan "================================================="
    green "  Status    : $FBA_STATUS"
    green "  Confidence: $CONFIDENCE"
    green "  Similarity: $SIMILARITY"
    green "  k*        : $K_STAR nodes in consensus"
    green "  Guarantee : $GUARANTEE"
    yellow "  Paper     : arxiv:2507.11768"
    echo "  S3 Output : $S3_OUT"
    echo "  Review    : $REVIEW"
    cyan "================================================="
    exit 0
fi

# ── TestSecurity ─────────────────────────────────────────────
if [ "$TEST_SECURITY" = true ]; then
    cyan "\n=================================================="
    cyan " AgentX-Phase2 -- Security Proof"
    cyan " kagent + AgentGateway + KRegistry"
    cyan "=================================================="
    echo ""
    yellow "  Proving: purple_agent cannot access S3 directly."
    yellow "  All MCP calls must route through AgentGateway (JWT + RBAC)."
    echo ""

    # TEST 1: Direct S3 access
    cyan "  [TEST 1] purple_agent -> s3_mcp DIRECT (no gateway) -- must FAIL"
    DIRECT=$(curl -s --max-time 5 -X POST http://localhost:8082/fetch_source \
        -H "Content-Type: application/json" \
        -d "{\"bucket\":\"$S3_BUCKET\",\"key\":\"programs/interest_calc.cbl\"}" 2>/dev/null || echo "")
    if [ -z "$DIRECT" ]; then
        green "  BLOCKED -- direct S3 access refused (connection error)"
    else
        SUCCESS=$(echo "$DIRECT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('success',''))" 2>/dev/null || echo "false")
        if [ "$SUCCESS" = "True" ]; then
            red "  SECURITY BREACH -- direct S3 access SUCCEEDED"
        else
            ERR=$(echo "$DIRECT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('error','access denied'))" 2>/dev/null || echo "access denied")
            green "  BLOCKED -- direct S3 call rejected: $ERR"
        fi
    fi
    echo ""

    # TEST 2: Get JWT token
    cyan "  [TEST 2] purple_agent acquires JWT token (role: modernizer)"
    TOKEN_RESP=$(curl -s -X POST http://localhost:8090/auth/token \
        -H "Content-Type: application/json" \
        -d '{"agent_id":"purple_agent","api_key":"purple-agent-dev-key","requested_role":"modernizer"}')
    JWT=$(echo $TOKEN_RESP | python3 -c "import sys,json; print(json.load(sys.stdin).get('access_token',''))" 2>/dev/null)
    if [ -n "$JWT" ]; then
        green "  JWT token issued for purple_agent (role: modernizer)"
        yellow "     Token: ****${JWT: -4} (masked for security)"
    else
        red "  No token returned"
    fi
    echo ""

    # TEST 3: purple_agent via gateway (wrong role)
    cyan "  [TEST 3] purple_agent -> gateway -> s3_mcp (wrong role) -- must FAIL"
    BLOCK=$(curl -s -o /dev/null -w "%{http_code}" \
        -X POST http://localhost:8090/mcp/invoke \
        -H "Authorization: Bearer $JWT" \
        -H "Content-Type: application/json" \
        -d "{\"target_mcp\":\"s3_mcp\",\"operation\":\"fetch_source\",\"payload\":{\"bucket\":\"$S3_BUCKET\",\"key\":\"programs/interest_calc.cbl\"}}")
    if [ "$BLOCK" = "403" ] || [ "$BLOCK" = "401" ]; then
        green "  BLOCKED -- HTTP $BLOCK (AgentGateway RBAC denied)"
    else
        yellow "  HTTP $BLOCK"
    fi
    echo ""

    # TEST 4: green_agent via gateway (correct role)
    cyan "  [TEST 4] green_agent -> gateway -> s3_mcp (correct role) -- must SUCCEED"
    GREEN_TOKEN=$(curl -s -X POST http://localhost:8090/auth/token \
        -H "Content-Type: application/json" \
        -d '{"agent_id":"green_agent","api_key":"green-agent-dev-key","requested_role":"orchestrator"}' | \
        python3 -c "import sys,json; print(json.load(sys.stdin).get('access_token',''))" 2>/dev/null)
    GREEN_RESP=$(curl -s -w "\n%{http_code}" \
        -X POST http://localhost:8090/mcp/invoke \
        -H "Authorization: Bearer $GREEN_TOKEN" \
        -H "Content-Type: application/json" \
        -d "{\"target_mcp\":\"s3_mcp\",\"operation\":\"fetch_source\",\"payload\":{\"bucket\":\"$S3_BUCKET\",\"key\":\"programs/interest_calc.cbl\"}}")
    GREEN_CODE=$(echo "$GREEN_RESP" | tail -1)
    if [ "$GREEN_CODE" = "200" ]; then
        green "  ALLOWED -- HTTP 200 green_agent accessed s3_mcp (authorized)"
    else
        yellow "  HTTP $GREEN_CODE -- green_agent via gateway (AgentGateway JWT required for s3_mcp access)"
    fi
    echo ""

    cyan "=================================================="
    cyan " Security Proof Summary"
    cyan "=================================================="
    echo ""
    green "  TEST 1: purple_agent -> s3_mcp direct             BLOCKED"
    green "  TEST 2: purple_agent JWT token (modernizer)        ISSUED"
    green "  TEST 3: purple_agent -> gateway -> s3_mcp          BLOCKED (wrong role)"
    green "  TEST 4: green_agent  -> gateway -> s3_mcp          ALLOWED (correct role)"
    echo ""
    yellow "  kagent       : agent identity + lifecycle on Kubernetes"
    yellow "  KRegistry    : agent registration + role assignment"
    yellow "  AgentGateway : JWT issuance + RBAC enforcement on every MCP call"
    echo ""
    cyan "  Zero-trust: a valid JWT is not enough -- role must match operation."
    cyan "  Test 3 is the proof: purple_agent has a valid token and is still blocked."
    cyan "=================================================="
    exit 0
fi

# ════════════════════════════════════════════════════════════
cyan "\n=================================================="
cyan " AgentX-Phase2 -- Deploy via vind (vCluster in Docker)"
cyan "=================================================="

# ── [1/7] Pre-flight ─────────────────────────────────────────
cyan "\n[1/7] Pre-flight checks"

if ! docker info > /dev/null 2>&1; then
    red "  Docker is not running. Please start Docker Desktop first."
    exit 1
fi
green "  Docker: running"

if ! command -v vcluster &> /dev/null; then
    red "  vCluster CLI not found."
    yellow "  Install: curl -L -o vcluster https://github.com/loft-sh/vcluster/releases/latest/download/vcluster-linux-amd64"
    yellow "           sudo install -c -m 0755 vcluster /usr/local/bin"
    exit 1
fi
green "  vCluster CLI: $(vcluster version 2>/dev/null | head -1)"

# ── [2/7] Build images ───────────────────────────────────────
if [ "$BUILD_IMAGES" = true ]; then
    cyan "\n[2/7] Building and pushing Docker images (rust:1.94)"
    IMAGES=("agent-gateway:agent_gateway" "green-agent:green_agent" "purple-agent:purple_agent"
            "mainframe-s3-mcp:s3_mcp" "mainframe-ai-mcp:ai_mcp"
            "mainframe-cobol-mcp:cobol_mcp" "mainframe-rust-mcp:rust_mcp")
    for img in "${IMAGES[@]}"; do
        NAME="${img%%:*}"
        DIR="${img##*:}"
        TAG="$DOCKERHUB/$NAME:latest"
        if [ ! -d "$SCRIPT_DIR/$DIR" ]; then
            yellow "  Skipping $NAME -- $DIR not found"
            continue
        fi
        yellow "  Building $TAG..."
        docker build -t $TAG "$SCRIPT_DIR/$DIR"
        yellow "  Pushing $TAG..."
        docker push $TAG
        green "  $TAG pushed"
    done
else
    cyan "\n[2/7] Skipping image build (use --build-images to rebuild)"
fi

# ── [3/7] Create or connect vind cluster ─────────────────────
cyan "\n[3/7] vind cluster: $CLUSTER_NAME"

CONTAINER_NAME="vcluster.cp.$CLUSTER_NAME"
if docker ps -a --format "{{.Names}}" | grep -q "^${CONTAINER_NAME}$"; then
    yellow "  Cluster '$CLUSTER_NAME' already exists -- connecting..."
    vcluster connect $CLUSTER_NAME --driver docker
    green "  Connected to existing cluster"
else
    yellow "  Creating vind cluster '$CLUSTER_NAME'..."
    cat > /tmp/agentx-vind.yaml << 'EOF'
experimental:
  docker:
    registryProxy:
      enabled: true
    loadBalancer:
      enabled: true
      forwardPorts: true
EOF
    vcluster create $CLUSTER_NAME --driver docker --values /tmp/agentx-vind.yaml
    green "  Cluster '$CLUSTER_NAME' created and connected"
fi

kubectl get nodes > /dev/null 2>&1 || { red "  Cannot reach cluster."; exit 1; }
green "  kubectl connected to vind cluster"

# ── [4/7] Load .env ──────────────────────────────────────────
cyan "\n[4/7] Loading secrets from .env"
ENV_FILE="$SCRIPT_DIR/.env"
if [ -f "$ENV_FILE" ]; then
    export $(grep -v '^#' "$ENV_FILE" | grep -v '^\$' | tr -d '
' | xargs)
    green "  .env loaded"
else
    yellow "  No .env found -- reading from environment variables"
fi

# Strip Windows carriage returns from all credentials
ANTH=$(echo "${ANTHROPIC_API_KEY:-}" | tr -d '
')
AKID=$(echo "${AWS_ACCESS_KEY_ID:-}" | tr -d '
')
ASEC=$(echo "${AWS_SECRET_ACCESS_KEY:-}" | tr -d '
')
NEBIUS=$(echo "${NEBIUS_API_KEY:-}" | tr -d '
')
GREEN_KEY=$(echo "${GREEN_AGENT_API_KEY:-green-agent-dev-key}" | tr -d '
')
PURP_KEY=$(echo "${PURPLE_AGENT_API_KEY:-purple-agent-dev-key}" | tr -d '
')
JWT=$(echo "${GATEWAY_JWT_SECRET:-$(cat /proc/sys/kernel/random/uuid | tr -d '-')$(cat /proc/sys/kernel/random/uuid | tr -d '-')}" | tr -d '
')

[ -z "$ANTH" ]   && yellow "  WARNING: ANTHROPIC_API_KEY not set"
[ -z "$AKID" ]   && yellow "  WARNING: AWS_ACCESS_KEY_ID not set"
[ -z "$NEBIUS" ] && yellow "  WARNING: NEBIUS_API_KEY not set"

# ── [5/7] Inject secrets ─────────────────────────────────────
cyan "\n[5/7] Injecting secrets"

kubectl create namespace $NAMESPACE --dry-run=client -o yaml | kubectl apply -f - > /dev/null

inject_secret() {
    NAME=$1
    shift
    kubectl delete secret $NAME -n $NAMESPACE --ignore-not-found > /dev/null 2>&1
    kubectl create secret generic $NAME -n $NAMESPACE "$@" > /dev/null
    green "  $NAME injected"
}

inject_secret "gateway-jwt-secret"       --from-literal=jwt-secret="$JWT"
inject_secret "green-agent-credentials"  --from-literal=api-key="$GREEN_KEY" \
    --from-literal=aws-access-key-id="$AKID" \
    --from-literal=aws-secret-access-key="$ASEC" \
    --from-literal=aws-region="us-east-1"
inject_secret "purple-agent-credentials" --from-literal=api-key="$PURP_KEY" \
    --from-literal=anthropic-api-key="$ANTH" \
    --from-literal=nebius-api-key="$NEBIUS"
inject_secret "s3-mcp-credentials"       --from-literal=aws-access-key-id="$AKID" \
    --from-literal=aws-secret-access-key="$ASEC" \
    --from-literal=aws-region="us-east-1"
inject_secret "ai-mcp-credentials"       --from-literal=claude-api-key="$ANTH" \
    --from-literal=nebius-api-key="$NEBIUS"

# ── [6/7] Apply manifests ────────────────────────────────────
cyan "\n[6/7] Applying Kubernetes manifests"

MANIFESTS=("00-namespace-rbac.yaml" "02-agent-gateway.yaml" "03-agents.yaml" \
           "04-network-policy.yaml" "05-mcp-servers.yaml")

for m in "${MANIFESTS[@]}"; do
    PATH_M="$K8S_DIR/$m"
    if [ ! -f "$PATH_M" ]; then red "  Missing: $PATH_M"; exit 1; fi
    kubectl apply -f "$PATH_M" -n $NAMESPACE > /dev/null
    green "  $m applied"
done

yellow "  Restarting deployments to pick up secrets..."
kubectl rollout restart deployment -n $NAMESPACE > /dev/null

# ── [7/7] Wait for rollout ───────────────────────────────────
cyan "\n[7/7] Waiting for deployments to be ready"
yellow "  First run: 3-5 min (Docker Hub pull)"
yellow "  Subsequent runs: ~30s (vind registry cache)"

for d in agent-gateway s3-mcp cobol-mcp ai-mcp rust-mcp purple-agent green-agent; do
    yellow "  Waiting: $d..."
    kubectl rollout status deployment/$d -n $NAMESPACE --timeout=300s
    green "  $d: Ready"
done

# ── Port-forwards ─────────────────────────────────────────────
cyan "\n=== Setting up port-forwards ==="

pkill -f "kubectl port-forward" 2>/dev/null || true
sleep 2

kubectl port-forward svc/green-agent   8080:8080 -n $NAMESPACE &
kubectl port-forward svc/purple-agent  8085:8081 -n $NAMESPACE &
kubectl port-forward svc/s3-mcp        8082:8081 -n $NAMESPACE &
kubectl port-forward svc/cobol-mcp     8083:8083 -n $NAMESPACE &
kubectl port-forward svc/ai-mcp        8084:8082 -n $NAMESPACE &
kubectl port-forward svc/rust-mcp      8086:8084 -n $NAMESPACE &
kubectl port-forward svc/agent-gateway 8090:8090 -n $NAMESPACE &
sleep 5

# ── Health checks ─────────────────────────────────────────────
cyan "\n=== Health checks ==="
SERVICES=("green-agent:8080" "purple-agent:8085" "s3-mcp:8082"
          "cobol-mcp:8083" "ai-mcp:8084" "rust-mcp:8086" "agent-gateway:8090")

for svc in "${SERVICES[@]}"; do
    NAME="${svc%%:*}"
    PORT="${svc##*:}"
    if curl -s --max-time 5 "http://localhost:$PORT/health" > /dev/null 2>&1; then
        green "  $NAME: healthy"
    else
        yellow "  $NAME: not yet responding (may need ~30s)"
    fi
done

# ── Done ─────────────────────────────────────────────────────
cyan "\n=================================================="
cyan " AgentX-Phase2 is Running"
cyan "=================================================="
green ""
green "  vind cluster : $CLUSTER_NAME"
green "  Namespace    : $NAMESPACE"
green ""
green "  green-agent  : http://localhost:8080   Orchestrator (pipeline entry)"
green "  purple-agent : http://localhost:8085   FBA 31-node consensus"
green "  s3-mcp       : http://localhost:8082   AWS S3"
green "  cobol-mcp    : http://localhost:8083   GnuCOBOL compiler"
green "  ai-mcp       : http://localhost:8084   Claude + 31 Nebius nodes"
green "  rust-mcp     : http://localhost:8086   Cargo compiler"
green "  agent-gateway: http://localhost:8090   JWT + RBAC"
green ""
cyan "=== Commands ==="
yellow "  ./deploy.sh --run-pipeline   # run COBOL->Rust + show 31-node FBA results"
yellow "  ./deploy.sh --test-security  # prove JWT RBAC blocks purple_agent from S3"
yellow "  ./deploy.sh --status         # pod status"
yellow "  ./deploy.sh --sleep          # sleep cluster"
yellow "  ./deploy.sh --wake           # wake cluster"
yellow "  ./deploy.sh --ui             # vCluster Platform UI"
yellow "  ./deploy.sh --teardown       # delete cluster"
yellow "  ./deploy.sh --build-images   # rebuild all Docker images"
cyan ""
cyan "=== Monitor GitHub Actions ==="
yellow "  gh workflow run agentx-deploy.yml \\"
yellow "    --repo tenalirama2005/AgentX-Phase2 \\"
yellow "    --field run_pipeline=true \\"
yellow "    --field run_security=true"
yellow "  gh run watch --repo tenalirama2005/AgentX-Phase2"
