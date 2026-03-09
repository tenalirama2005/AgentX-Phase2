# test_purple_agent.ps1
# Quick smoke test for purple_agent
# Run after: cargo run (or docker run)

$BASE_URL = "http://localhost:8081"

Write-Host "🟣 Purple Agent Test Suite" -ForegroundColor Magenta
Write-Host "================================`n"

# ── Test 1: Health check ──────────────────────────────────────────────────────
Write-Host "Test 1: Health Check" -ForegroundColor Cyan
$health = Invoke-RestMethod -Uri "$BASE_URL/health" -Method GET
Write-Host "  Status:  $($health.status)"
Write-Host "  Agent:   $($health.agent) v$($health.version)"
Write-Host "  Paper:   $($health.paper_reference)"
Write-Host ""

# ── Test 2: Config ────────────────────────────────────────────────────────────
Write-Host "Test 2: Config" -ForegroundColor Cyan
$config = Invoke-RestMethod -Uri "$BASE_URL/config" -Method GET
Write-Host "  Primary:    $($config.model_primary)"
Write-Host "  Secondary:  $($config.model_secondary)"
Write-Host "  Similarity: $($config.similarity_threshold)"
Write-Host "  Confidence: $($config.confidence_threshold)"
Write-Host "  ε:          $($config.epsilon)"
Write-Host "  Θ:          $($config.theta)"
Write-Host ""

# ── Test 3: Modernize (the real deal) ────────────────────────────────────────
Write-Host "Test 3: POST /modernize (COBOL → Rust via FBA)" -ForegroundColor Cyan

$cobolSource = @"
       IDENTIFICATION DIVISION.
       PROGRAM-ID. INTEREST-CALC.
       DATA DIVISION.
       WORKING-STORAGE SECTION.
       01 WS-PRINCIPAL    PIC 9(7)V99.
       01 WS-RATE         PIC 9(3)V99.
       01 WS-INTEREST     PIC 9(7)V99.
       PROCEDURE DIVISION.
           MOVE 10000.00 TO WS-PRINCIPAL
           MOVE 5.50 TO WS-RATE
           COMPUTE WS-INTEREST = WS-PRINCIPAL * WS-RATE / 100
           DISPLAY 'CALCULATED INTEREST: ' WS-INTEREST
           STOP RUN.
"@

$body = @{
    cobol_source = $cobolSource
    epsilon = 0.01
} | ConvertTo-Json

Write-Host "  Sending COBOL (interest_calc)..."
$start = Get-Date

try {
    $result = Invoke-RestMethod `
        -Uri "$BASE_URL/modernize" `
        -Method POST `
        -Body $body `
        -ContentType "application/json" `
        -TimeoutSec 120

    $elapsed = ((Get-Date) - $start).TotalSeconds

    Write-Host ""
    Write-Host "  ┌─────────────────────────────────────────" -ForegroundColor Green
    Write-Host "  │ STATUS:    $($result.status)" -ForegroundColor Green
    Write-Host "  │ Action:    $($result.action)" -ForegroundColor Green
    Write-Host "  │ k*:        $($result.k_star)" -ForegroundColor Green
    Write-Host "  │ Confidence: $($result.confidence)" -ForegroundColor Green
    Write-Host "  │ Similarity: $($result.semantic_similarity)" -ForegroundColor Green
    Write-Host "  │ Bayesian:  $($result.bayesian_guarantee)" -ForegroundColor Green
    Write-Host "  │ Martingale: $($result.martingale_satisfied)" -ForegroundColor Green
    Write-Host "  │ Paper:     $($result.paper_reference)" -ForegroundColor Green
    Write-Host "  │ Elapsed:   ${elapsed}s" -ForegroundColor Green
    Write-Host "  └─────────────────────────────────────────" -ForegroundColor Green
    Write-Host ""

    if ($result.rust_code) {
        Write-Host "  Generated Rust Code:" -ForegroundColor Yellow
        Write-Host "  ─────────────────────"
        Write-Host $result.rust_code -ForegroundColor White
    } else {
        Write-Host "  ⚠️  No Rust code (disagreement or error)" -ForegroundColor Yellow
        Write-Host "  FBA Node results:"
        foreach ($node in $result.fba_details.node_results) {
            Write-Host "    $($node.node_id): confidence=$($node.confidence)"
        }
    }

} catch {
    Write-Host "  ❌ Request failed: $_" -ForegroundColor Red
}

Write-Host "`n✅ Test suite complete" -ForegroundColor Green
