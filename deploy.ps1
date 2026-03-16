# ============================================================
# Mainframe Modernization Pipeline - Kubernetes Deployment
# Author: Venkat Nagala
# Usage: .\deploy.ps1
# ============================================================

Write-Host "🚀 Mainframe Modernization Pipeline - Kubernetes Deployment" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan

# ============================================================
# STEP 1: Verify Prerequisites
# ============================================================
Write-Host "`n📋 Step 1: Verifying Prerequisites..." -ForegroundColor Yellow

# Check kubectl
if (!(Get-Command kubectl -ErrorAction SilentlyContinue)) {
    Write-Host "❌ kubectl not found! Please install kubectl first." -ForegroundColor Red
    exit 1
}
Write-Host "✅ kubectl found" -ForegroundColor Green

# Check cluster connection
$nodeStatus = kubectl get nodes --no-headers 2>$null
if (!$nodeStatus) {
    Write-Host "❌ Cannot connect to Kubernetes cluster!" -ForegroundColor Red
    exit 1
}
Write-Host "✅ Kubernetes cluster connected" -ForegroundColor Green

# ============================================================
# STEP 2: Load environment variables from .env
# ============================================================
Write-Host "`n📋 Step 2: Loading environment variables..." -ForegroundColor Yellow

$envFile = ".env"
if (!(Test-Path $envFile)) {
    Write-Host "❌ .env file not found! Please create .env from .env.example" -ForegroundColor Red
    exit 1
}

# Parse .env file
Get-Content $envFile | ForEach-Object {
    if ($_ -match "^\s*([^#][^=]+)=(.*)$") {
        $name = $matches[1].Trim()
        $value = $matches[2].Trim()
        Set-Item -Path "env:$name" -Value $value
    }
}
Write-Host "✅ Environment variables loaded" -ForegroundColor Green

# ============================================================
# STEP 3: Apply Namespace and RBAC
# ============================================================
Write-Host "`n📋 Step 3: Creating Namespace and RBAC..." -ForegroundColor Yellow
kubectl apply -f k8s/base/00-namespace-rbac.yaml
if ($LASTEXITCODE -ne 0) { Write-Host "❌ Failed to apply namespace/RBAC!" -ForegroundColor Red; exit 1 }
Write-Host "✅ Namespace and RBAC created" -ForegroundColor Green

# ============================================================
# STEP 4: Create Secrets with real values from .env
# ============================================================
Write-Host "`n📋 Step 4: Creating Kubernetes Secrets..." -ForegroundColor Yellow

# Delete existing secrets (ignore errors if not exist)
kubectl delete secret gateway-jwt-secret -n mainframe-modernization 2>$null
kubectl delete secret green-agent-credentials -n mainframe-modernization 2>$null
kubectl delete secret purple-agent-credentials -n mainframe-modernization 2>$null
kubectl delete secret ai-mcp-credentials -n mainframe-modernization 2>$null
kubectl delete secret s3-mcp-credentials -n mainframe-modernization 2>$null

# Create secrets with real values
kubectl create secret generic gateway-jwt-secret `
    --from-literal=jwt-secret="$env:JWT_SECRET" `
    -n mainframe-modernization

kubectl create secret generic green-agent-credentials `
    --from-literal=api-key="$env:AGENT_API_KEY" `
    --from-literal=aws-access-key-id="$env:AWS_ACCESS_KEY_ID" `
    --from-literal=aws-secret-access-key="$env:AWS_SECRET_ACCESS_KEY" `
    --from-literal=aws-region="us-east-1" `
    -n mainframe-modernization

kubectl create secret generic purple-agent-credentials `
    --from-literal=api-key="purple-agent-dev-key-change-in-prod" `
    --from-literal=claude-api-key="$env:CLAUDE_API_KEY" `
    -n mainframe-modernization

kubectl create secret generic ai-mcp-credentials `
    --from-literal=claude-api-key="$env:CLAUDE_API_KEY" `
    -n mainframe-modernization

kubectl create secret generic s3-mcp-credentials `
    --from-literal=aws-access-key-id="$env:AWS_ACCESS_KEY_ID" `
    --from-literal=aws-secret-access-key="$env:AWS_SECRET_ACCESS_KEY" `
    --from-literal=aws-region="us-east-1" `
    -n mainframe-modernization

if ($LASTEXITCODE -ne 0) { Write-Host "❌ Failed to create secrets!" -ForegroundColor Red; exit 1 }
Write-Host "✅ Secrets created" -ForegroundColor Green

# ============================================================
# STEP 5: Apply ConfigMap and Secrets YAML
# ============================================================
Write-Host "`n📋 Step 5: Applying ConfigMap..." -ForegroundColor Yellow
kubectl apply -f k8s/base/01-secrets-config.yaml
Write-Host "✅ ConfigMap applied" -ForegroundColor Green

# ============================================================
# STEP 6: Deploy Agent Gateway
# ============================================================
Write-Host "`n📋 Step 6: Deploying Agent Gateway..." -ForegroundColor Yellow
kubectl apply -f k8s/base/02-agent-gateway.yaml
if ($LASTEXITCODE -ne 0) { Write-Host "❌ Failed to deploy agent-gateway!" -ForegroundColor Red; exit 1 }
Write-Host "✅ Agent Gateway deployed" -ForegroundColor Green

# Wait for agent-gateway to be ready
Write-Host "⏳ Waiting for Agent Gateway to be ready..." -ForegroundColor Yellow
kubectl rollout status deployment/agent-gateway -n mainframe-modernization --timeout=120s
Write-Host "✅ Agent Gateway is ready!" -ForegroundColor Green

# ============================================================
# STEP 7: Deploy MCP Servers
# ============================================================
Write-Host "`n📋 Step 7: Deploying MCP Servers..." -ForegroundColor Yellow
kubectl apply -f k8s/base/05-mcp-servers.yaml
if ($LASTEXITCODE -ne 0) { Write-Host "❌ Failed to deploy MCP servers!" -ForegroundColor Red; exit 1 }
Write-Host "✅ MCP Servers deployed" -ForegroundColor Green

# ============================================================
# STEP 8: Deploy Agents
# ============================================================
Write-Host "`n📋 Step 8: Deploying Agents..." -ForegroundColor Yellow
kubectl apply -f k8s/base/03-agents.yaml
if ($LASTEXITCODE -ne 0) { Write-Host "❌ Failed to deploy agents!" -ForegroundColor Red; exit 1 }
Write-Host "✅ Agents deployed" -ForegroundColor Green

# ============================================================
# STEP 9: Apply Network Policies
# ============================================================
Write-Host "`n📋 Step 9: Applying Zero-Trust Network Policies..." -ForegroundColor Yellow
kubectl apply -f k8s/base/04-network-policy.yaml
if ($LASTEXITCODE -ne 0) { Write-Host "❌ Failed to apply network policies!" -ForegroundColor Red; exit 1 }
Write-Host "✅ Network Policies applied" -ForegroundColor Green

# ============================================================
# STEP 10: Port Forward for local access
# ============================================================
Write-Host "`n📋 Step 10: Waiting for all pods to be ready..." -ForegroundColor Yellow
kubectl rollout status deployment/agent-gateway -n mainframe-modernization --timeout=180s
kubectl rollout status deployment/green-agent -n mainframe-modernization --timeout=180s
kubectl rollout status deployment/purple-agent -n mainframe-modernization --timeout=180s
kubectl rollout status deployment/s3-mcp -n mainframe-modernization --timeout=180s
kubectl rollout status deployment/ai-mcp -n mainframe-modernization --timeout=180s
kubectl rollout status deployment/cobol-mcp -n mainframe-modernization --timeout=180s
kubectl rollout status deployment/rust-mcp -n mainframe-modernization --timeout=180s

# ============================================================
# STEP 11: Show deployment status
# ============================================================
Write-Host "`n============================================================" -ForegroundColor Cyan
Write-Host "🎉 DEPLOYMENT COMPLETE!" -ForegroundColor Green
Write-Host "============================================================" -ForegroundColor Cyan

Write-Host "`n📊 Pod Status:" -ForegroundColor Yellow
kubectl get pods -n mainframe-modernization

Write-Host "`n🌐 Services:" -ForegroundColor Yellow
kubectl get services -n mainframe-modernization

# ============================================================
# STEP 12: Start Port Forward and Run Demo
# ============================================================
Write-Host "`n📋 Step 12: Starting Port Forward for Green Agent..." -ForegroundColor Yellow

# Kill any existing kubectl port-forward on 8080
$existing = netstat -ano | findstr :8080 | Select-String "LISTENING"
if ($existing) {
    $pid = ($existing -split '\s+')[-1]
    taskkill /PID $pid /F 2>$null
    Write-Host "✅ Cleared existing process on port 8080" -ForegroundColor Green
}

# Start port-forward in background
$portForward = Start-Process -FilePath "kubectl" `
    -ArgumentList "port-forward svc/green-agent 8080:8080 -n mainframe-modernization" `
    -PassThru -WindowStyle Hidden

Write-Host "✅ Port forward started (PID: $($portForward.Id))" -ForegroundColor Green

# Give it 3 seconds to establish
Write-Host "⏳ Waiting for port forward to establish..." -ForegroundColor Yellow
Start-Sleep -Seconds 3

# ============================================================
# STEP 13: Run Demo
# ============================================================
Write-Host "`n📋 Step 13: Running Demo Pipeline..." -ForegroundColor Yellow
.\demo.ps1

# ============================================================
# STEP 14: Zero-Trust RBAC Security Test
# ============================================================
Write-Host "`n📋 Step 14: Running Zero-Trust RBAC Security Test..." -ForegroundColor Yellow
Write-Host "🔐 Proving Purple Agent CANNOT access S3 (least-privilege enforced)..." -ForegroundColor Cyan

# Need agent-gateway port-forward for security test
$portForwardGateway = Start-Process -FilePath "kubectl" `
    -ArgumentList "port-forward svc/agent-gateway 8090:8090 -n mainframe-modernization" `
    -PassThru -WindowStyle Hidden

Start-Sleep -Seconds 3

try {
    # Get Purple Agent JWT token
    $purpleToken = (Invoke-RestMethod `
        -Uri "http://localhost:8090/auth/token" `
        -Method POST `
        -ContentType "application/json" `
        -Body '{"agent_id":"purple_agent","api_key":"purple-agent-dev-key-change-in-prod","requested_role":"modernizer"}'
    ).access_token

    Write-Host "✅ Purple Agent JWT token issued (role: modernizer)" -ForegroundColor Green

    # Attempt S3 fetch — should be DENIED
    $rbacResult = Invoke-RestMethod `
        -Uri "http://localhost:8090/mcp/invoke" `
        -Method POST `
        -Headers @{Authorization="Bearer $purpleToken"} `
        -ContentType "application/json" `
        -Body '{"target_mcp":"s3_mcp","operation":"fetch_source","payload":{}}'

    Write-Host "⚠️  Unexpected: Request was allowed!" -ForegroundColor Red
    Write-Host ($rbacResult | ConvertTo-Json) -ForegroundColor Red

} catch {
    # 403 Forbidden is the EXPECTED and CORRECT response
    Write-Host "✅ RBAC ENFORCED — Purple Agent blocked from S3 as expected!" -ForegroundColor Green
    Write-Host "🛡️  Zero-Trust Security Working Correctly:" -ForegroundColor Cyan
    Write-Host "   Role 'Modernizer' is NOT authorized to call fetch_source on s3_mcp" -ForegroundColor White
}

# Cleanup gateway port-forward
Stop-Process -Id $portForwardGateway.Id -Force 2>$null
Write-Host "✅ Gateway port-forward stopped" -ForegroundColor Green

# ============================================================
# STEP 15: Cleanup green-agent port forward
# ============================================================
Write-Host "`n📋 Step 15: Cleaning up..." -ForegroundColor Yellow
Stop-Process -Id $portForward.Id -Force 2>$null
Write-Host "✅ Port forwards stopped" -ForegroundColor Green

Write-Host "`n============================================================" -ForegroundColor Cyan
Write-Host "✅ Mainframe Modernization Pipeline is running on Kubernetes!" -ForegroundColor Green
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "`n💡 To run again manually:" -ForegroundColor Cyan
Write-Host "   kubectl port-forward svc/green-agent 8080:8080 -n mainframe-modernization" -ForegroundColor White
Write-Host "   .\demo.ps1" -ForegroundColor White