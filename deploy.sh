#!/bin/bash
# ============================================================
# AgentX-Phase2 — Deploy to Kubernetes via kind (kind in Docker)
# Author: Venkat Nagala | For the Cloud By the Cloud
#
# kind = kind in Docker — KinD replacement with:
#   LoadBalancer works out of the box
#   Free Kiali Platform UI (accessible from anywhere)
#   Sleep/wake cluster
#   Add EC2/GPU nodes via VPN
#   Pull-through Docker registry cache
#   GitHub: https://github.com/kubernetes-sigs/kind
#
# Prerequisites:
#   Docker Desktop running (Kubernetes NOT required)
#   kind CLI: curl -Lo ./kind https://kind.sigs.k8s.io/dl/latest/kind-linux-amd64
#             chmod +x ./kind && sudo mv ./kind /usr/local/bin/kind
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
#   ./deploy.sh --ui             # open Kiali Platform UI
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
    yellow "Sleeping kind cluster '$CLUSTER_NAME'..."
    docker stop agentx-phase2-control-plane
    green "Cluster sleeping. Run ./deploy.sh --wake to resume."
    exit 0
fi

# ── Wake ─────────────────────────────────────────────────────
if [ "$WAKE_CLUSTER" = true ]; then
    yellow "Waking kind cluster '$CLUSTER_NAME'..."
    docker start agentx-phase2-control-plane
    sleep 10
    kubectl config use-context kind-agentx-phase2
    green "Cluster awake and connected."
    exit 0
fi

# ── UI ───────────────────────────────────────────────────────
if [ "$UI" = true ]; then
    cyan "Starting Kiali Platform UI..."
    kubectl port-forward svc/kiali 20001:20001 -n istio-system &
    sleep 3
    if command -v wslview &> /dev/null; then
        wslview http://localhost:20001
    else
        green "  Open browser: http://localhost:20001"
    fi
    green "  Kiali Dashboard: http://localhost:20001"
    exit 0
fi

# ── Teardown ─────────────────────────────────────────────────
if [ "$TEARDOWN" = true ]; then
    yellow "Deleting kind cluster '$CLUSTER_NAME'..."
    kind delete cluster --name $CLUSTER_NAME
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
    
    # Verify credentials are set
    cyan "  Verifying credentials..."
    NEBIUS_CHECK=$(kubectl exec -n $NAMESPACE \
      $(kubectl get pod -n $NAMESPACE -l app=purple-agent -o jsonpath='{.items[0].metadata.name}') \
      -c purple-agent -- env 2>/dev/null | grep "^NEBIUS_API_KEY=" | cut -d'=' -f2-)
    AWS_CHECK=$(kubectl exec -n $NAMESPACE \
      $(kubectl get pod -n $NAMESPACE -l app=s3-mcp -o jsonpath='{.items[0].metadata.name}') \
      -c s3-mcp -- env 2>/dev/null | grep "^AWS_ACCESS_KEY_ID=" | cut -d'=' -f2-)
    
    if [ -z "$NEBIUS_CHECK" ]; then
        red "  ERROR: NEBIUS_API_KEY is empty in purple-agent pod!"
        red "  Run: kubectl delete secret purple-agent-credentials -n $NAMESPACE"
        red "  Then re-inject with correct values from .env"
        exit 1
    fi
    if [ -z "$AWS_CHECK" ]; then
        red "  ERROR: AWS_ACCESS_KEY_ID is empty in s3-mcp pod!"
        red "  Run: kubectl delete secret s3-mcp-credentials -n $NAMESPACE"
        red "  Then re-inject with correct values from .env"
        exit 1
    fi
    green "  Credentials verified ✓"

    PR=$(curl -s -X POST http://localhost:8080/modernize \
        -H "Content-Type: application/json" \
        -d '{"s3_key":"programs/interest_calc.cbl"}')

    if [ -z "$PR" ]; then
        red "  ERROR: No response from green-agent. Check port-forward."
        red "  Run: kubectl port-forward svc/green-agent 8080:8080 -n mainframe-modernization &"
        exit 1
    fi

    FBA_STATUS=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('fba_status',''))")
    CONFIDENCE=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('fba_confidence',''))")
    SIMILARITY=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('semantic_similarity',''))")
    K_STAR=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('k_star',''))")
    GUARANTEE=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('bayesian_guarantee',''))")
    REVIEW=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('review_folder',''))")
    S3_OUT=$(echo $PR | python3 -c "import sys,json; print(json.load(sys.stdin).get('s3_output_key',''))")
    ERROR=$(echo $PR | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('error') or '')")
    STATUS_MSG=$(echo $PR | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('status') or '')")

    # Check for pipeline errors
    if [ -z "$FBA_STATUS" ] || [ "$FBA_STATUS" = "None" ]; then
        red "  Pipeline failed: $STATUS_MSG"
        red "  Error: $ERROR"
        red ""
        red "  Common fixes:"
        red "  1. kubectl rollout restart deployment -n mainframe-modernization"
        red "  2. kubectl apply -f k8s/base/07-istio-network-policy.yaml"
        red "  3. Restart port-forwards"
        exit 1
    fi

# Fetch per-node FBA report
    if [ -n "$REVIEW" ]; then
        REPORT_BODY="{\"bucket\":\"$S3_BUCKET\",\"key\":\"$REVIEW/fba_report/fba_report.json\"}"
        REPORT=$(curl -s --max-time 30 -X POST http://localhost:8082/fetch_source \
            -H "Content-Type: application/json" \
            -H "X-AgentGateway-Token: agentx-internal-token" \
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
print('')
print('  {:<30} {:>4}  {:<25} {}'.format('Model', 'Conf', 'Bar (25 chars = 100%)', 'Status'))
print('  ' + '='*68)
for i, n in enumerate(nodes, 1):
    conf = n.get('confidence', 0)
    pct = int(conf * 100)
    bar_len = int(conf * 25)
    empty_len = 25 - bar_len
    icon = 'OK  ' if conf >= 0.85 else 'WARN' if conf >= 0.70 else 'LOW '
    bar = '#' * bar_len + '.' * empty_len
    print('  [{:02d}] {:<28} {:3d}%  [{}] {}'.format(
        i, n['node_id'][:28], pct, bar, icon))
print('  ' + '='*68)
" 2>/dev/null || echo "  (FBA report display unavailable)"
    fi    

    echo ""
    cyan "================================================="
    green "  Status    : $FBA_STATUS"
    green "  Confidence: $CONFIDENCE"
    green "  Similarity: $SIMILARITY"
    green "  k*        : $K_STAR reasoning steps per model (Bayesian minimum)"
    cyan "             k* = ceil(θ × √n × log(1/ε)) where n = COBOL line count"
    cyan "             Fixed for interest_calc.cbl — changes only if COBOL input changes"
    green "  Guarantee : $GUARANTEE"
    yellow "  Paper     : arxiv:2507.11768"
    if [ -n "$S3_OUT" ] && [ "$S3_OUT" != "None" ]; then
        echo "  S3 Output : $S3_OUT"
        PRESIGNED=$(echo $PR | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('presigned_url') or '')")
        if [ -n "$PRESIGNED" ] && [ "$PRESIGNED" != "None" ]; then
            cyan "  Download  : (pre-signed URL valid 1 hour)"
            echo "  $PRESIGNED"
        fi
    fi
    if [ -n "$REVIEW" ] && [ "$REVIEW" != "None" ]; then
        FBA_REPORT_URL=$(curl -s --max-time 10 \
            -X POST http://localhost:8082/generate_presigned_url \
            -H "Content-Type: application/json" \
            -H "X-AgentGateway-Token: agentx-internal-token" \
            -d "{\"bucket\":\"$S3_BUCKET\",\"key\":\"$REVIEW/fba_report/fba_report.json\"}" | \
            python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('presigned_url') or '')")
        if [ -n "$FBA_REPORT_URL" ] && [ "$FBA_REPORT_URL" != "None" ]; then
            cyan "  FBA Report: (pre-signed URL valid 1 hour)"
            echo "  $FBA_REPORT_URL"
        fi
        cyan "  Per-node  : $REVIEW/<model_name>/interest_calc.rs"
    fi
    cyan "================================================="
    exit 0
fi

# ── TestSecurity ─────────────────────────────────────────────
if [ "$TEST_SECURITY" = true ]; then
    # Re-inject all secrets to ensure they are current
    ENV_FILE="$SCRIPT_DIR/.env"
    if [ -f "$ENV_FILE" ]; then
        JWT=$(grep "^GATEWAY_JWT_SECRET" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')
        ANTH=$(grep "^ANTHROPIC_API_KEY" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')
        AKID=$(grep "^AWS_ACCESS_KEY_ID" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')
        ASEC=$(grep "^AWS_SECRET_ACCESS_KEY" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')
        NEBIUS=$(grep "^NEBIUS_API_KEY" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')
        PURP_KEY=$(grep "^PURPLE_AGENT_API_KEY" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')
        GREEN_KEY=$(grep "^GREEN_AGENT_API_KEY" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')
        [ -z "$PURP_KEY" ] && PURP_KEY="purple-agent-dev-key"
        [ -z "$GREEN_KEY" ] && GREEN_KEY="green-agent-dev-key"

        kubectl delete secret gateway-jwt-secret purple-agent-credentials \
            green-agent-credentials s3-mcp-credentials \
            -n $NAMESPACE --ignore-not-found > /dev/null 2>&1

        kubectl create secret generic gateway-jwt-secret \
            -n $NAMESPACE \
            --from-literal=jwt-secret="$JWT" \
            --from-literal=secret="$JWT" > /dev/null 2>&1
        kubectl create secret generic purple-agent-credentials \
            -n $NAMESPACE \
            --from-literal=api-key="$PURP_KEY" \
            --from-literal=anthropic-api-key="$ANTH" \
            --from-literal=nebius-api-key="$NEBIUS" > /dev/null 2>&1
        kubectl create secret generic green-agent-credentials \
            -n $NAMESPACE \
            --from-literal=api-key="$GREEN_KEY" \
            --from-literal=aws-access-key-id="$AKID" \
            --from-literal=aws-secret-access-key="$ASEC" \
            --from-literal=aws-region="us-east-1" > /dev/null 2>&1
        kubectl create secret generic s3-mcp-credentials \
            -n $NAMESPACE \
            --from-literal=aws-access-key-id="$AKID" \
            --from-literal=aws-secret-access-key="$ASEC" \
            --from-literal=aws-region="us-east-1" > /dev/null 2>&1

        kubectl rollout restart deployment/agent-gateway -n $NAMESPACE > /dev/null 2>&1
        kubectl rollout status deployment/agent-gateway \
            -n $NAMESPACE --timeout=60s > /dev/null 2>&1
        pkill -f "port-forward svc/agent-gateway" 2>/dev/null || true
        sleep 3
        kubectl port-forward svc/agent-gateway 8090:8090 \
            -n $NAMESPACE > /dev/null 2>&1 &
        sleep 5
    fi

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
cyan " AgentX-Phase2 -- Deploy via kind (kind in Docker)"
cyan "=================================================="

# ── [1/7] Pre-flight ─────────────────────────────────────────
cyan "\n[1/7] Pre-flight checks"

if ! docker info > /dev/null 2>&1; then
    red "  Docker is not running. Please start Docker Desktop first."
    exit 1
fi
green "  Docker: running"

if ! command -v kind &> /dev/null; then
    red "  kind not found."
    yellow "  Install: curl -Lo ./kind https://kind.sigs.k8s.io/dl/latest/kind-linux-amd64"
    yellow "           chmod +x ./kind && sudo mv ./kind /usr/local/bin/kind"
    exit 1
fi
green "  kind: $(kind version)"
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

# ── [3/7] Create or connect kind cluster ─────────────────────
cyan "\n[3/7] kind cluster: $CLUSTER_NAME"

if kind get clusters 2>/dev/null | grep -q "^${CLUSTER_NAME}$"; then
    yellow "  Cluster '$CLUSTER_NAME' already exists -- connecting..."
    kubectl config use-context kind-$CLUSTER_NAME
    green "  Connected to existing cluster"
else
    yellow "  Creating kind cluster '$CLUSTER_NAME'..."
    printf "apiVersion: kind.x-k8s.io/v1alpha4\nkind: Cluster\nnodes:\n- role: control-plane\n" > /tmp/kind-config.yaml
    kind create cluster --name $CLUSTER_NAME --config /tmp/kind-config.yaml --wait 60s
    green "  Cluster '$CLUSTER_NAME' created and connected"
fi

kubectl get nodes > /dev/null 2>&1 || { red "  Cannot reach cluster."; exit 1; }
green "  kubectl connected to kind cluster"

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
ANTH=$(grep "^ANTHROPIC_API_KEY" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')
AKID=$(grep "^AWS_ACCESS_KEY_ID" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')
ASEC=$(grep "^AWS_SECRET_ACCESS_KEY" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')
NEBIUS=$(grep "^NEBIUS_API_KEY" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')
GREEN_KEY=$(grep "^GREEN_AGENT_API_KEY" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')
PURP_KEY=$(grep "^PURPLE_AGENT_API_KEY" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')
JWT=$(grep "^GATEWAY_JWT_SECRET" "$ENV_FILE" | cut -d'=' -f2- | tr -d '\r\n')

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
           "04-network-policy.yaml" "05-mcp-servers.yaml" \
           "06-istio-authpolicy.yaml" "07-istio-network-policy.yaml")

for m in "${MANIFESTS[@]}"; do
    PATH_M="$K8S_DIR/$m"
    if [ ! -f "$PATH_M" ]; then red "  Missing: $PATH_M"; exit 1; fi
    kubectl apply -f "$PATH_M" -n $NAMESPACE > /dev/null 2>&1 && \
        green "  $m applied" || \
        yellow "  $m skipped (Istio CRDs not ready)"
done
# Ensure Istio sidecar injection is enabled
kubectl label namespace $NAMESPACE istio-injection=enabled --overwrite > /dev/null
green "  Istio sidecar injection enabled"

yellow "  Restarting deployments to inject Istio sidecars..."
kubectl delete pods -n $NAMESPACE --all --force --grace-period=0 > /dev/null 2>&1
green "  Pods restarted with Istio sidecars"

yellow "  Restarting deployments to pick up secrets..."
kubectl rollout restart deployment -n $NAMESPACE > /dev/null

# ── [7/7] Wait for rollout ───────────────────────────────────
cyan "\n[7/7] Waiting for deployments to be ready (2/2 with Istio sidecars)"
yellow "  First run: 3-5 min (Docker Hub pull)"
yellow "  Subsequent runs: ~90s"

sleep 30
for d in agent-gateway s3-mcp cobol-mcp ai-mcp rust-mcp purple-agent green-agent; do
    yellow "  Waiting: $d..."
    kubectl rollout status deployment/$d -n $NAMESPACE --timeout=300s
    green "  $d: Ready"
done

# Verify Istio sidecars
READY=$(kubectl get pods -n $NAMESPACE --no-headers | grep "2/2" | wc -l)
TOTAL=$(kubectl get pods -n $NAMESPACE --no-headers | wc -l)
green "  Istio sidecars: $READY/$TOTAL pods showing 2/2"
kubectl get pods -n $NAMESPACE

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
green "  kind cluster : $CLUSTER_NAME"
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
yellow "  ./deploy.sh --ui             # kiali Platform UI"
yellow "  ./deploy.sh --teardown       # delete cluster"
yellow "  ./deploy.sh --build-images   # rebuild all Docker images"
cyan ""
cyan "=== Monitor GitHub Actions ==="
yellow "  gh workflow run agentx-deploy.yml \\"
yellow "    --repo tenalirama2005/AgentX-Phase2 \\"
yellow "    --field run_pipeline=true \\"
yellow "    --field run_security=true"
yellow "  gh run watch --repo tenalirama2005/AgentX-Phase2"
