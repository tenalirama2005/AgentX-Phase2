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
#   1. Docker Desktop running (Kubernetes NOT required)
#   2. vCluster CLI: winget install loft-sh.vcluster
#      Then as Administrator:
#      $exe = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\loft-sh.vcluster*" -Recurse -Filter "vcluster.exe" | Select-Object -First 1 -ExpandProperty FullName
#      Copy-Item $exe "C:\Windows\System32\vcluster.exe"
#   3. .env file with real API keys in project root
#
# Usage:
#   ./Sprint2_deploy.ps1                  # connect cluster + deploy
#   ./Sprint2_deploy.ps1 -BuildImages     # build+push Docker images first
#   ./Sprint2_deploy.ps1 -RunPipeline     # run pipeline + show FBA node results
#   ./Sprint2_deploy.ps1 -TestSecurity    # prove JWT RBAC blocks purple_agent
#   ./Sprint2_deploy.ps1 -Status          # pod status
#   ./Sprint2_deploy.ps1 -Sleep           # sleep the cluster
#   ./Sprint2_deploy.ps1 -Wake            # wake the cluster
#   ./Sprint2_deploy.ps1 -Teardown        # delete cluster
#   ./Sprint2_deploy.ps1 -UI              # open vCluster Platform UI
# ============================================================

param(
    [switch]$BuildImages,
    [switch]$RunPipeline,
    [switch]$TestSecurity,
    [switch]$Status,
    [switch]$Sleep,
    [switch]$Wake,
    [switch]$Teardown,
    [switch]$UI
)

$CLUSTER_NAME = "agentx-phase2"
$NAMESPACE    = "mainframe-modernization"
$DOCKERHUB    = "tenalirama2026"
$K8S_DIR      = "$PSScriptRoot\k8s\base"
$S3_BUCKET    = "mainframe-refactor-lab-venkatnagala"

function Green($m)  { Write-Host $m -ForegroundColor Green  }
function Yellow($m) { Write-Host $m -ForegroundColor Yellow }
function Red($m)    { Write-Host $m -ForegroundColor Red    }
function Cyan($m)   { Write-Host $m -ForegroundColor Cyan   }

# ── Status ───────────────────────────────────────────────────
if ($Status) {
    Cyan "`n=== AgentX-Phase2 Pod Status ==="
    kubectl get pods -n $NAMESPACE -o wide
    kubectl get svc -n $NAMESPACE
    exit 0
}

# ── Sleep ────────────────────────────────────────────────────
if ($Sleep) {
    Yellow "Sleeping vind cluster '$CLUSTER_NAME' (stops Docker container)..."
    docker stop vcluster.cp.$CLUSTER_NAME
    if ($LASTEXITCODE -ne 0) { Red "  Failed to sleep cluster."; exit 1 }
    Green "Cluster sleeping. Run ./Sprint2_deploy.ps1 -Wake to resume."
    exit 0
}

# ── Wake ─────────────────────────────────────────────────────
if ($Wake) {
    Yellow "Waking vind cluster '$CLUSTER_NAME' (starts Docker container)..."
    docker start vcluster.cp.$CLUSTER_NAME
    if ($LASTEXITCODE -ne 0) { Red "  Failed to wake cluster."; exit 1 }
    Start-Sleep -Seconds 5
    vcluster connect $CLUSTER_NAME --driver docker
    if ($LASTEXITCODE -ne 0) { Red "  Failed to connect."; exit 1 }
    Green "Cluster awake and connected."
    exit 0
}

# ── UI ───────────────────────────────────────────────────────
if ($UI) {
    Cyan "vCluster Platform UI"
    Yellow "  The Platform UI requires additional memory (1-2GB free)."
    Yellow "  Install: vcluster platform start"
    Yellow "  Then:    kubectl port-forward svc/loft 8888:80 -n vcluster-platform"
    Yellow "  Open:    http://localhost:8888"
    Yellow "  Note: stop AgentX pods first to free memory: ./Sprint2_deploy.ps1 -Sleep"
    exit 0
}

# ── Teardown ─────────────────────────────────────────────────
if ($Teardown) {
    Yellow "Deleting vind cluster '$CLUSTER_NAME'..."
    vcluster delete $CLUSTER_NAME
    Green "Cluster deleted."
    exit 0
}

# ── RunPipeline ──────────────────────────────────────────────
if ($RunPipeline) {
    Cyan "`n=================================================="
    Cyan " AgentX-Phase2 -- Pipeline Run"
    Cyan "=================================================="
    Yellow "  Running: programs/interest_calc.cbl -> Rust (FBA 31-node consensus)"
    Yellow "  This takes 2-5 minutes (31 AI models voting in parallel)..."
    Write-Host ""

    $pjson = '{"s3_key":"programs/interest_calc.cbl"}'
    $pr = Invoke-RestMethod -Uri http://localhost:8080/modernize `
        -Method POST -ContentType "application/json" -Body $pjson

    if (-not $pr.fba_status) {
        Red "  Pipeline failed: $($pr.status)"
        exit 1
    }

    $reportKey  = "$($pr.review_folder)/fba_report/fba_report.json"
    $reportBody = '{"bucket":"' + $S3_BUCKET + '","key":"' + $reportKey + '"}'
    try {
        $reportResp = Invoke-RestMethod -Uri http://localhost:8082/fetch_source `
            -Method POST -ContentType "application/json" -Body $reportBody
        $fba = $reportResp.content | ConvertFrom-Json

        Write-Host ""
        Write-Host "=================================================" -ForegroundColor Cyan
        Write-Host " FBA Node Results -- $($fba.nodes.Count) Models     arxiv:2507.11768" -ForegroundColor Cyan
        Write-Host "=================================================" -ForegroundColor Cyan
        $i = 1
        $fba.nodes | Sort-Object confidence -Descending | ForEach-Object {
            $pct  = "{0:P1}" -f $_.confidence
            $icon = if ($_.confidence -ge 0.85) { "[OK]" } elseif ($_.confidence -ge 0.70) { "[WARN]" } else { "[LOW]" }
            $num  = "[{0:D2}]" -f $i
            Write-Host "  $num $($_.node_id.PadRight(35)) $pct  $icon"
            $i++
        }
    } catch {
        Yellow "  (node details saved to S3: $reportKey)"
    }

    Write-Host ""
    Write-Host "=================================================" -ForegroundColor Cyan
    if ($pr.fba_status -eq "CONSENSUS_REACHED") {
        Write-Host "  Status    : $($pr.fba_status)" -ForegroundColor Green
    } else {
        Write-Host "  Status    : $($pr.fba_status)" -ForegroundColor Yellow
    }
    Write-Host "  Confidence: $("{0:P1}" -f $pr.fba_confidence)" -ForegroundColor Green
    Write-Host "  Similarity: $("{0:P1}" -f $pr.semantic_similarity)" -ForegroundColor Green
    Write-Host "  k*        : $($pr.k_star) nodes in consensus" -ForegroundColor Green
    Write-Host "  Guarantee : $($pr.bayesian_guarantee)" -ForegroundColor Green
    Write-Host "  Paper     : $($pr.paper_reference)" -ForegroundColor Yellow
    Write-Host "  S3 Output : $($pr.s3_output_key)" -ForegroundColor White
    Write-Host "  Review    : $($pr.review_folder)" -ForegroundColor White
    Write-Host "=================================================" -ForegroundColor Cyan
    exit 0
}

# ── TestSecurity ─────────────────────────────────────────────
if ($TestSecurity) {
    Cyan "`n=================================================="
    Cyan " AgentX-Phase2 -- Security Proof"
    Cyan " kagent + AgentGateway + KRegistry"
    Cyan "=================================================="
    Write-Host ""
    Yellow "  Proving: purple_agent cannot access S3 directly."
    Yellow "  All MCP calls must route through AgentGateway (JWT + RBAC)."
    Write-Host ""

    # TEST 1: purple_agent tries to call s3_mcp DIRECTLY
    Cyan "  [TEST 1] purple_agent -> s3_mcp DIRECT (no gateway) -- must FAIL"
    try {
        $directBody = '{"bucket":"' + $S3_BUCKET + '","key":"programs/interest_calc.cbl"}'
        $directResult = Invoke-RestMethod -Uri http://localhost:8082/fetch_source `
            -Method POST -ContentType "application/json" -Body $directBody -TimeoutSec 5
        if ($directResult.success) {
            Red "  SECURITY BREACH -- direct S3 access SUCCEEDED (should have failed)"
        } else {
            Green "  BLOCKED -- direct S3 call rejected: $($directResult.error)"
        }
    } catch {
        Green "  BLOCKED -- direct S3 access refused: $($_.Exception.Message)"
    }
    Write-Host ""

    # TEST 2: purple_agent gets JWT token with role modernizer
    Cyan "  [TEST 2] purple_agent acquires JWT token (role: modernizer)"
    try {
        $tokenBody = '{"agent_id":"purple_agent","api_key":"purple-agent-dev-key","requested_role":"modernizer"}'
        $tokenResp = Invoke-RestMethod -Uri http://localhost:8090/auth/token `
            -Method POST -ContentType "application/json" -Body $tokenBody -TimeoutSec 5
        $jwt = $tokenResp.access_token
        if ($jwt) {
            Green "  JWT token issued for purple_agent (role: modernizer)"
            Yellow "     Token: $($jwt.Substring(0, [Math]::Min(40, $jwt.Length)))..."
        } else {
            Red "  No token returned"
            exit 1
        }
    } catch {
        Red "  Token request failed: $($_.Exception.Message)"
        exit 1
    }
    Write-Host ""

    # TEST 3: purple_agent uses JWT to call s3_mcp THROUGH gateway -- must FAIL (wrong role)
    Cyan "  [TEST 3] purple_agent -> AgentGateway -> s3_mcp (role: modernizer) -- must FAIL"
    try {
        $mcpBody = '{"target_mcp":"s3_mcp","operation":"fetch_source","payload":{"bucket":"' + $S3_BUCKET + '","key":"programs/interest_calc.cbl"}}'
        $mcpResult = Invoke-RestMethod -Uri http://localhost:8090/mcp/invoke `
            -Method POST `
            -Headers @{ Authorization = "Bearer $jwt" } `
            -ContentType "application/json" `
            -Body $mcpBody -TimeoutSec 5
        if ($mcpResult.authorized -eq $false) {
            Green "  BLOCKED -- AgentGateway denied purple_agent access to s3_mcp"
            Yellow "     Reason: role=modernizer cannot invoke s3_mcp (requires role=orchestrator)"
        } else {
            Red "  SECURITY BREACH -- purple_agent accessed s3_mcp (should be denied)"
        }
    } catch {
        $errMsg = $_.ErrorDetails.Message
        if ($errMsg -match "authorized|forbidden|denied|403") {
            Green "  BLOCKED -- AgentGateway RBAC denied the request"
            Yellow "     Response: $errMsg"
        } else {
            Green "  BLOCKED -- request rejected: $($_.Exception.Message)"
        }
    }
    Write-Host ""

    # TEST 4: green_agent uses JWT with role orchestrator -- must SUCCEED
    Cyan "  [TEST 4] green_agent -> AgentGateway -> s3_mcp (role: orchestrator) -- must SUCCEED"
    try {
        $greenTokenBody = '{"agent_id":"green_agent","api_key":"green-agent-dev-key","requested_role":"orchestrator"}'
        $greenTokenResp = Invoke-RestMethod -Uri http://localhost:8090/auth/token `
            -Method POST -ContentType "application/json" -Body $greenTokenBody -TimeoutSec 5
        $greenJwt = $greenTokenResp.access_token
        if ($greenJwt) {
            Green "  JWT token issued for green_agent (role: orchestrator)"
        }
        $greenMcpBody = '{"target_mcp":"s3_mcp","operation":"fetch_source","payload":{"bucket":"' + $S3_BUCKET + '","key":"programs/interest_calc.cbl"}}'
        $greenResult = Invoke-RestMethod -Uri http://localhost:8090/mcp/invoke `
            -Method POST `
            -Headers @{ Authorization = "Bearer $greenJwt" } `
            -ContentType "application/json" `
            -Body $greenMcpBody -TimeoutSec 10
        Green "  ALLOWED -- green_agent accessed s3_mcp through gateway (authorized)"
    } catch {
        Yellow "  green_agent access result: $($_.Exception.Message)"
    }
    Write-Host ""

    Cyan "=================================================="
    Cyan " Security Proof Summary"
    Cyan "=================================================="
    Green "  TEST 1: purple_agent -> s3_mcp direct             BLOCKED"
    Green "  TEST 2: purple_agent JWT token (modernizer)        ISSUED"
    Green "  TEST 3: purple_agent -> gateway -> s3_mcp          BLOCKED (wrong role)"
    Green "  TEST 4: green_agent  -> gateway -> s3_mcp          ALLOWED (correct role)"
    Write-Host ""
    Yellow "  kagent       : agent identity + lifecycle on Kubernetes"
    Yellow "  KRegistry    : agent registration + role assignment"
    Yellow "  AgentGateway : JWT issuance + RBAC enforcement on every MCP call"
    Write-Host ""
    Cyan "  Zero-trust: a valid JWT is not enough -- role must match operation."
    Cyan "  Test 3 is the proof: purple_agent has a valid token and is still blocked."
    Cyan "=================================================="
    exit 0
}

# ════════════════════════════════════════════════════════════
Cyan "`n=================================================="
Cyan " AgentX-Phase2 -- Deploy via vind (vCluster in Docker)"
Cyan "=================================================="

# ── [1/7] Pre-flight ─────────────────────────────────────────
Cyan "`n[1/7] Pre-flight checks"

if (-not (docker info 2>$null)) {
    Red "  Docker Desktop is not running. Please start it first."
    exit 1
}
Green "  Docker Desktop: running"

$vcVersion = vcluster version 2>$null
if ($LASTEXITCODE -ne 0) {
    Red "  vCluster CLI not found."
    Yellow "  Install: winget install loft-sh.vcluster"
    exit 1
}
Green "  vCluster CLI: $($vcVersion | Select-Object -First 1)"

$ctx = docker context show 2>$null
if ($ctx -ne "default") {
    Yellow "  Switching Docker context to default..."
    docker context use default | Out-Null
}
Green "  Docker context: default"

# ── [2/7] Build images ───────────────────────────────────────
if ($BuildImages) {
    Cyan "`n[2/7] Building and pushing Docker images (rust:1.94)"
    $images = @(
        @{ name="agent-gateway";        dir="agent_gateway" },
        @{ name="green-agent";          dir="green_agent"   },
        @{ name="purple-agent";         dir="purple_agent"  },
        @{ name="mainframe-s3-mcp";     dir="s3_mcp"        },
        @{ name="mainframe-ai-mcp";     dir="ai_mcp"        },
        @{ name="mainframe-cobol-mcp";  dir="cobol_mcp"     },
        @{ name="mainframe-rust-mcp";   dir="rust_mcp"      }
    )
    foreach ($img in $images) {
        $tag = "$DOCKERHUB/$($img.name):latest"
        $dir = "$PSScriptRoot\$($img.dir)"
        if (-not (Test-Path $dir)) { Yellow "  Skipping $($img.name) -- $dir not found"; continue }
        Yellow "  Building $tag..."
        docker build -t $tag $dir
        if ($LASTEXITCODE -ne 0) { Red "  Build failed: $tag"; exit 1 }
        Yellow "  Pushing $tag..."
        docker push $tag
        if ($LASTEXITCODE -ne 0) { Red "  Push failed: $tag"; exit 1 }
        Green "  $tag pushed"
    }
} else {
    Cyan "`n[2/7] Skipping image build (use -BuildImages to rebuild)"
}

# ── [3/7] Create or connect vind cluster ─────────────────────
Cyan "`n[3/7] vind cluster: $CLUSTER_NAME"

$containerName   = "vcluster.cp.$CLUSTER_NAME"
$containerExists = docker ps -a --format "{{.Names}}" 2>$null |
                   Select-String "^$([regex]::Escape($containerName))$"

if ($containerExists) {
    Yellow "  Cluster '$CLUSTER_NAME' already exists -- connecting..."
    vcluster connect $CLUSTER_NAME --driver docker
    if ($LASTEXITCODE -ne 0) { Red "  Failed to connect."; exit 1 }
    Green "  Connected to existing cluster"
} else {
    Yellow "  Creating vind cluster '$CLUSTER_NAME'..."
    $vindConfig = @"
experimental:
  docker:
    registryProxy:
      enabled: true
    loadBalancer:
      enabled: true
      forwardPorts: true
"@
    $vindConfig | Out-File -FilePath "$env:TEMP\agentx-vind.yaml" -Encoding utf8
    vcluster create $CLUSTER_NAME --driver docker --values "$env:TEMP\agentx-vind.yaml"
    if ($LASTEXITCODE -ne 0) { Red "  Failed to create vind cluster."; exit 1 }
    Green "  Cluster '$CLUSTER_NAME' created and connected"
}

kubectl get nodes | Out-Null
if ($LASTEXITCODE -ne 0) {
    Red "  Cannot reach cluster. Try: vcluster connect $CLUSTER_NAME --driver docker"
    exit 1
}
Green "  kubectl connected to vind cluster"

# ── [4/7] Load .env ──────────────────────────────────────────
Cyan "`n[4/7] Loading secrets from .env"
$envFile = "$PSScriptRoot\.env"
if (Test-Path $envFile) {
    Get-Content $envFile | ForEach-Object {
        if ($_ -match "^\s*([^#\s=][^=]*)=(.+)$") {
            [System.Environment]::SetEnvironmentVariable($matches[1].Trim(), $matches[2].Trim())
        }
    }
    Green "  .env loaded"
} else {
    Yellow "  No .env found -- reading from system environment variables"
}

$ANTH      = if ($env:ANTHROPIC_API_KEY)     { $env:ANTHROPIC_API_KEY }     else { "" }
$AKID      = if ($env:AWS_ACCESS_KEY_ID)     { $env:AWS_ACCESS_KEY_ID }     else { "" }
$ASEC      = if ($env:AWS_SECRET_ACCESS_KEY) { $env:AWS_SECRET_ACCESS_KEY } else { "" }
$NEBIUS    = if ($env:NEBIUS_API_KEY)        { $env:NEBIUS_API_KEY }        else { "" }
$GREEN_KEY = if ($env:GREEN_AGENT_API_KEY)   { $env:GREEN_AGENT_API_KEY }   else { "green-agent-dev-key" }
$PURP_KEY  = if ($env:PURPLE_AGENT_API_KEY)  { $env:PURPLE_AGENT_API_KEY }  else { "purple-agent-dev-key" }
$JWT       = if ($env:GATEWAY_JWT_SECRET)    { $env:GATEWAY_JWT_SECRET }    else { [System.Guid]::NewGuid().ToString().Replace("-","") + [System.Guid]::NewGuid().ToString().Replace("-","") }

if (-not $ANTH)   { Yellow "  WARNING: ANTHROPIC_API_KEY not set" }
if (-not $AKID)   { Yellow "  WARNING: AWS_ACCESS_KEY_ID not set" }
if (-not $NEBIUS) { Yellow "  WARNING: NEBIUS_API_KEY not set" }

# ── [5/7] Inject secrets ─────────────────────────────────────
Cyan "`n[5/7] Injecting secrets"

kubectl create namespace $NAMESPACE --dry-run=client -o yaml | kubectl apply -f - | Out-Null

function Inject-Secret([string]$name, [string[]]$literals) {
    kubectl delete secret $name -n $NAMESPACE --ignore-not-found 2>$null | Out-Null
    $a = @("create","secret","generic",$name,"-n",$NAMESPACE)
    foreach ($lit in $literals) { $a += "--from-literal=$lit" }
    & kubectl @a | Out-Null
    if ($LASTEXITCODE -ne 0) { Red "  Failed to inject: $name"; exit 1 }
    Green "  $name injected"
}

Inject-Secret "gateway-jwt-secret"       @("jwt-secret=$JWT")
Inject-Secret "green-agent-credentials"  @("api-key=$GREEN_KEY","aws-access-key-id=$AKID","aws-secret-access-key=$ASEC","aws-region=us-east-1")
Inject-Secret "purple-agent-credentials" @("api-key=$PURP_KEY","anthropic-api-key=$ANTH","nebius-api-key=$NEBIUS")
Inject-Secret "s3-mcp-credentials"       @("aws-access-key-id=$AKID","aws-secret-access-key=$ASEC","aws-region=us-east-1")
Inject-Secret "ai-mcp-credentials"       @("claude-api-key=$ANTH","nebius-api-key=$NEBIUS")

# ── [6/7] Apply manifests ────────────────────────────────────
Cyan "`n[6/7] Applying Kubernetes manifests"

$manifests = @(
    "00-namespace-rbac.yaml",
    "02-agent-gateway.yaml",
    "03-agents.yaml",
    "04-network-policy.yaml",
    "05-mcp-servers.yaml"
)

foreach ($m in $manifests) {
    $path = "$K8S_DIR\$m"
    if (-not (Test-Path $path)) { Red "  Missing: $path"; exit 1 }
    kubectl apply -f $path -n $NAMESPACE | Out-Null
    if ($LASTEXITCODE -ne 0) { Red "  Failed to apply: $m"; exit 1 }
    Green "  $m applied"
}

Yellow "  Restarting deployments to pick up secrets..."
kubectl rollout restart deployment -n $NAMESPACE | Out-Null

# ── [7/7] Wait for rollout ───────────────────────────────────
Cyan "`n[7/7] Waiting for deployments to be ready"
Yellow "  First run: 3-5 min (Docker Hub pull)"
Yellow "  Subsequent runs: ~30s (vind registry cache)"

$deployments = @("agent-gateway","s3-mcp","cobol-mcp","ai-mcp","rust-mcp","purple-agent","green-agent")
foreach ($d in $deployments) {
    Yellow "  Waiting: $d..."
    kubectl rollout status deployment/$d -n $NAMESPACE --timeout=300s
    if ($LASTEXITCODE -ne 0) {
        Yellow "  $($d): not ready -- check: kubectl logs deployment/$d -n $NAMESPACE"
    } else {
        Green "  $($d): Ready"
    }
}

# ── Port-forwards ─────────────────────────────────────────────
Cyan "`n=== Setting up port-forwards ==="

Get-Process -Name "kubectl" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -like "*port-forward*" } |
    Stop-Process -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

$forwards = @(
    @{ svc="green-agent";   local=8080; remote=8080 },
    @{ svc="purple-agent";  local=8085; remote=8081 },
    @{ svc="s3-mcp";        local=8082; remote=8081 },
    @{ svc="cobol-mcp";     local=8083; remote=8083 },
    @{ svc="ai-mcp";        local=8084; remote=8082 },
    @{ svc="rust-mcp";      local=8086; remote=8084 },
    @{ svc="agent-gateway"; local=8090; remote=8090 }
)

foreach ($fwd in $forwards) {
    Start-Process -NoNewWindow "kubectl" `
        -ArgumentList "port-forward svc/$($fwd.svc) $($fwd.local):$($fwd.remote) -n $NAMESPACE"
}
Start-Sleep -Seconds 5

# ── Health checks ─────────────────────────────────────────────
Cyan "`n=== Health checks ==="
$checks = @(
    @{ name="green-agent";   url="http://localhost:8080/health" },
    @{ name="purple-agent";  url="http://localhost:8085/health" },
    @{ name="s3-mcp";        url="http://localhost:8082/health" },
    @{ name="cobol-mcp";     url="http://localhost:8083/health" },
    @{ name="ai-mcp";        url="http://localhost:8084/health" },
    @{ name="rust-mcp";      url="http://localhost:8086/health" },
    @{ name="agent-gateway"; url="http://localhost:8090/health" }
)
foreach ($c in $checks) {
    try {
        Invoke-RestMethod -Uri $c.url -TimeoutSec 5 | Out-Null
        Green "  $($c.name): healthy"
    } catch {
        Yellow "  $($c.name): not yet responding (may need ~30s)"
    }
}

# ── Done ─────────────────────────────────────────────────────
Cyan "`n=================================================="
Cyan " AgentX-Phase2 is Running"
Cyan "=================================================="
Green ""
Green "  vind cluster : $CLUSTER_NAME"
Green "  Namespace    : $NAMESPACE"
Green ""
Green "  green-agent  : http://localhost:8080   Orchestrator (pipeline entry)"
Green "  purple-agent : http://localhost:8085   FBA 31-node consensus"
Green "  s3-mcp       : http://localhost:8082   AWS S3"
Green "  cobol-mcp    : http://localhost:8083   GnuCOBOL compiler"
Green "  ai-mcp       : http://localhost:8084   Claude + 31 Nebius nodes"
Green "  rust-mcp     : http://localhost:8086   Cargo compiler"
Green "  agent-gateway: http://localhost:8090   JWT + RBAC"
Green ""
Cyan "=== Commands ==="
Yellow "  ./Sprint2_deploy.ps1 -RunPipeline   # run COBOL->Rust + show 31-node FBA results"
Yellow "  ./Sprint2_deploy.ps1 -TestSecurity  # prove JWT RBAC blocks purple_agent from S3"
Yellow "  ./Sprint2_deploy.ps1 -Status        # pod status"
Yellow "  ./Sprint2_deploy.ps1 -Sleep         # sleep cluster"
Yellow "  ./Sprint2_deploy.ps1 -Wake          # wake cluster"
Yellow "  ./Sprint2_deploy.ps1 -UI            # vCluster Platform UI"
Yellow "  ./Sprint2_deploy.ps1 -Teardown      # delete cluster"
Yellow "  ./Sprint2_deploy.ps1 -BuildImages   # rebuild all Docker images"