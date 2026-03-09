# 1. Configuration
$TaskID = "MODERN-DEMO-2026"
$S3Bucket = "mainframe-refactor-lab-venkatnagala"  # <--- UPDATE THIS
$S3Key = "programs/interest_calc.cbl"

# 2. Build the Payload
$Payload = @{
    task_id = $TaskID
    source_location = @{
        bucket = $S3Bucket
        key    = $S3Key
    }
} | ConvertTo-Json

# 3. Trigger the Green Agent
Write-Host "Sending Modernization Task to Green Agent..." -ForegroundColor Cyan
try {
    $result = Invoke-RestMethod `
        -Uri "http://127.0.0.1:8080/evaluate" `
        -Method Post `
        -Body $Payload `
        -ContentType "application/json"
    Write-Host "Task accepted! Monitor your Docker logs for the ASCII Verification report." -ForegroundColor Green
    Write-Host $result
    Start-Process $result.rust_code_url
} catch {
    Write-Host "❌ Modernization pipeline failed!" -ForegroundColor Red
    Write-Host "Error: $($_.Exception.Message)" -ForegroundColor Yellow
    Write-Host "Details: $($_.ErrorDetails)" -ForegroundColor Yellow
    Write-Host "`n💡 Troubleshooting:" -ForegroundColor Cyan
    Write-Host "   - Check pods: kubectl get pods -n mainframe-modernization" -ForegroundColor White
    Write-Host "   - Check logs: kubectl logs deployment/green-agent -n mainframe-modernization" -ForegroundColor White
}