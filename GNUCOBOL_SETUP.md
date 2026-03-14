# GnuCOBOL Setup Guide — AgentX Phase 2

## Why GnuCOBOL Is Required

AgentX Phase 2 modernizes **real COBOL mainframe programs** into Rust.
Before the AI pipeline can translate COBOL → Rust, it must **compile and execute**
the original COBOL program to capture its exact runtime output.
This output becomes the **ground truth** that the Rust translation must match.

```
interest_calc.cbl
      │
      ▼
 cobol_mcp (port 8083)
      │  calls cobc.exe  (GnuCOBOL compiler)
      │  calls compiled  .exe (runtime loads DLLs from gnucobol\tools\bin)
      │  captures stdout → ground truth
      ▼
 purple_agent FBA
      │  Claude + 30 Nebius models translate COBOL → Rust
      │  consensus validated against COBOL ground truth
      ▼
 modernized/<uuid>/interest_calc.rs  (saved to S3)
```

**Two failure points if GnuCOBOL is not configured correctly:**
- `cobc` not in PATH → Step 2 fails at compile time
- `gnucobol\tools\bin` not in PATH → compiled COBOL exe crashes at runtime (missing DLL)

Both must be set. This guide covers both.

---

## Installation: Chocolatey (Recommended)

AgentX Phase 2 was developed and validated with GnuCOBOL installed via
**Chocolatey** — the Windows package manager.

> ### ⚠️ Windows 11 Requirement: CMD as Administrator
> GnuCOBOL **must** be installed from a **Command Prompt (cmd) opened as
> Administrator** — not a regular terminal, not PowerShell alone.
> Windows 11 User Account Control (UAC) blocks the Chocolatey installer
> and the GnuCOBOL package from writing to `C:\ProgramData\` and setting
> System environment variables unless elevated via cmd.
>
> **How to open CMD as Administrator:**
> 1. Press `Win` key → type `cmd`
> 2. Right-click **Command Prompt** → **"Run as administrator"**
> 3. Click **Yes** on the UAC prompt
> 4. The title bar will show **Administrator: Command Prompt**
>
> All installation commands below must be run in this window.

### Step 1 — Install Chocolatey

In the **Administrator: Command Prompt** window:

```cmd
@"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" ^
  -NoProfile -InputFormat None -ExecutionPolicy Bypass ^
  -Command "iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))"
```

Verify:
```cmd
choco --version
```

### Step 2 — Install GnuCOBOL

Still in the **Administrator: Command Prompt** window:

```cmd
choco install gnucobol -y
```

Installs to: `C:\ProgramData\chocolatey\lib\gnucobol\tools\`

Chocolatey **automatically** sets these environment variables:

| Variable | Value set by Chocolatey |
|---|---|
| `COB_CONFIG_DIR` | `...\tools\config` |
| `COB_CFLAGS` | `-I"...\tools\include"` |
| `COB_LIBRARY_PATH` | `...\tools\lib` |
| `COB_LIBS` | `...\tools\lib\libcob.lib` |

### Step 3 — ⚠️ CRITICAL: Add gnucobol\tools\bin to System PATH

The Chocolatey installer does **not** add the runtime DLL folder to System PATH.
This is a **mandatory manual step** — without it, compiled COBOL programs
crash immediately at runtime with a missing DLL error.

**The exact path to add:**
```
C:\ProgramData\chocolatey\lib\gnucobol\tools\bin
```

**CMD as Administrator (same window used for installation):**
```cmd
setx /M PATH "%PATH%;C:\ProgramData\chocolatey\lib\gnucobol\tools\bin"
```

> `setx /M` writes to **System** (Machine-level) PATH — the `/M` flag is
> what requires Administrator. Without `/M` it only sets User PATH, which
> background services like `cobol_mcp` cannot see.

**Or via PowerShell as Administrator:**
```powershell
$gnuPath = "C:\ProgramData\chocolatey\lib\gnucobol\tools\bin"
$current = [System.Environment]::GetEnvironmentVariable("Path", "Machine")

if ($current -notlike "*gnucobol*") {
    [System.Environment]::SetEnvironmentVariable("Path", "$current;$gnuPath", "Machine")
    Write-Host "✅ GnuCOBOL runtime added to System PATH" -ForegroundColor Green
} else {
    Write-Host "✅ Already in System PATH" -ForegroundColor DarkGray
}
```

**Or via Windows GUI:**
1. `Win + R` → `sysdm.cpl` → Enter
2. **Advanced** tab → **Environment Variables**
3. **System variables** → select `Path` → **Edit**
4. **New** → paste `C:\ProgramData\chocolatey\lib\gnucobol\tools\bin`
5. **OK** → **OK** → **OK**

> ⚠️ Close ALL open terminals after this step.
> PATH changes only take effect in newly opened terminals and services.

### Step 4 — Verify

Open a **new** PowerShell window:

```powershell
# Confirm cobc is reachable
cobc --version

# Confirm the correct path is in System PATH
[System.Environment]::GetEnvironmentVariable("Path", "Machine") -split ";" |
    Where-Object { $_ -like "*gnucobol*" }
```

Expected:
```
cobc (GnuCOBOL) 3.2.0
...
C:\ProgramData\chocolatey\lib\gnucobol\tools\bin
```

---

## What Is in gnucobol\tools\bin

```
C:\ProgramData\chocolatey\lib\gnucobol\tools\
├── bin\                     ← ⭐ ADD THIS TO System PATH
│   ├── cobc.exe             ← COBOL compiler (called by cobol_mcp)
│   ├── cobcrun.exe          ← COBOL runner
│   ├── libcob-4.dll         ← GnuCOBOL runtime — loaded by compiled .exe
│   ├── libgmp-10.dll        ← GMP arbitrary precision (COMP-3 arithmetic)
│   └── [other runtime DLLs]
├── config\                  ← COB_CONFIG_DIR
├── include\                 ← COB_CFLAGS
└── lib\                     ← COB_LIBRARY_PATH / COB_LIBS
```

**Why the DLLs matter:**
`cobc` compiles COBOL to a native `.exe` that dynamically links `libcob-4.dll`
at runtime. Windows searches PATH for DLLs. Without `tools\bin` in PATH,
the exe exits immediately with:
```
The program can't start because libcob-4.dll is missing from your computer.
```
This causes `cobol_mcp` to report `success: false` with no output — and the
pipeline stops at Step 2 with no Rust code produced.

---

## Smoke Test

Run before starting the AgentX pipeline:

```powershell
$cobol = @"
       IDENTIFICATION DIVISION.
       PROGRAM-ID. SMOKE-TEST.
       PROCEDURE DIVISION.
           DISPLAY "GnuCOBOL OK".
           STOP RUN.
"@

$cbl = "$env:TEMP\smoke_test.cbl"
$exe = "$env:TEMP\smoke_test"
$cobol | Out-File -FilePath $cbl -Encoding ascii

cobc -x -o $exe $cbl
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Compile failed — check COB_CONFIG_DIR" -ForegroundColor Red; exit 1
}

$out = & "$exe.exe"
if ($out -match "GnuCOBOL OK") {
    Write-Host "✅ GnuCOBOL smoke test PASSED — pipeline ready" -ForegroundColor Green
} else {
    Write-Host "❌ Runtime failed — libcob-4.dll missing from PATH?" -ForegroundColor Red
}
Remove-Item $cbl, "$exe.exe" -ErrorAction SilentlyContinue
```

---

## Pipeline Health Check

After running `Sprint2_deploy.ps1`:

```powershell
Invoke-RestMethod http://localhost:8083/health | ConvertTo-Json
```

```json
{
  "status":             "healthy",
  "service":            "cobol_mcp",
  "version":            "1.0.0",
  "gnucobol_available": true
}
```

If `gnucobol_available: false` → PATH was not set before `cobol_mcp` started
→ fix PATH → restart `cobol_mcp`.

---

## COMP-3 Packed Decimal — Automatic Handling

IBM z/OS COBOL uses COMP-3 (packed decimal BCD) for financial fields.
GnuCOBOL on x86 converts COMP-3 to IEEE 754 float, producing inconsistent
results versus mainframe hardware BCD.

AgentX `cobol_mcp` normalizes COMP-3 automatically before every compile:

```rust
// cobol_mcp/src/main.rs → normalize_comp3()
// Replaces COMP-3 / COMPUTATIONAL-3 with standard DISPLAY
// Result: 200/200 consistent numeric outputs — no manual COBOL editing needed
// Reference: arxiv:2507.11768
```

---

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `cobc: command not found` | Terminal not restarted after PATH change | Open new terminal |
| `libcob-4.dll is missing` | `tools\bin` not in **System** PATH | Add `C:\ProgramData\chocolatey\lib\gnucobol\tools\bin` to System PATH |
| `No such file: default.conf` | `COB_CONFIG_DIR` not set | Re-run `choco install gnucobol -y` |
| `gnucobol_available: false` | Service started before PATH was set | Restart `cobol_mcp` |
| PATH set but still not working | Added to **User** PATH, not System PATH | Move to **System variables** — services use System PATH |

---

## Linux / macOS

```bash
# Ubuntu/Debian
sudo apt-get install -y gnucobol

# macOS
brew install gnucobol

# Verify
cobc --version
```

No DLL or PATH configuration needed — `ldconfig` handles library paths.

---

## Judge's Setup Checklist

```
□ Win key → type "cmd" → Right-click → "Run as administrator" → Yes
□ In Administrator CMD: choco install gnucobol -y
□ In Administrator CMD: setx /M PATH "%PATH%;C:\ProgramData\chocolatey\lib\gnucobol\tools\bin"
□ Close all terminals → open new terminal
□ cobc --version  →  confirms 3.2.x
□ Run smoke test  →  "✅ GnuCOBOL smoke test PASSED"
□ Populate D:\AgentX-Phase2\.env  (API keys)
□ cd D:\AgentX-Phase2 && .\Sprint2_deploy.ps1
□ GET http://localhost:8083/health  →  gnucobol_available: true
□ POST http://localhost:8080/modernize {"s3_key": "programs/interest_calc.cbl"}
```

**Tested configuration: GnuCOBOL 3.2.0 via Chocolatey, Windows 11**

---

*AgentX Phase 2 — COBOL-to-Rust Modernization Pipeline*
*arxiv:2507.11768 — Bayesian-in-Realization FBA Guarantee*
*Venkateshwar Rao Nagala — tenalirama2026@gmail.com*
