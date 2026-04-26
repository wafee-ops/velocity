# Engine Benchmark Report

Generated on: 2026-04-26T16:43:04.026Z
Strategy: `velocity`
Target Marker: `__VELOCITY_EXIT__`

## Metrics
- **Grade:** 100.0/100
- **Latency:** 939.5ms
- **Leaks:** 0

## Category Breakdown
| Category | Avg Grade | Avg Time |
|----------|-----------|----------|
| SYSTEM | 100.0 | 809.7ms |
| DEV | 100.0 | 765.8ms |
| SANDBOX | 100.0 | 1317.0ms |
| LOGIC | 100.0 | 2015.7ms |
| FILL | 100.0 | 911.7ms |

## Samples
| Command | Exit | Time | Grade | Notes |
|---------|------|------|-------|-------|
| `Write-Output Engine Check` | 0 | 1656ms | 100 | Passed |
| `$env:USERNAME` | 0 | 826ms | 100 | Passed |
| `$env:COMPUTERNAME` | 0 | 553ms | 100 | Passed |
| `$PSVersionTable.PSVersion.ToString()` | 0 | 474ms | 100 | Passed |
| `Get-Location | Select-Object -ExpandProperty Path` | 0 | 707ms | 100 | Passed |
| `Get-Item Env:PATH | Select-Object -ExpandProperty Value` | 0 | 642ms | 100 | Passed |
| `node -v` | 0 | 477ms | 100 | Passed |
| `npm -v` | 0 | 1581ms | 100 | Passed |
| `git --version` | 0 | 680ms | 100 | Passed |
| `git status` | 0 | 514ms | 100 | Passed |
| `cargo --version` | 0 | 577ms | 100 | Passed |
| `Get-ChildItem -LiteralPath "C:\Users\wafee\Documents\codex-projects\velocity\temp_sandbox" -Force | Select-Object -First 5 | ForEach-Object { $_.Name }` | 0 | 896ms | 100 | Passed |
| `Set-Location -LiteralPath "C:\Users\wafee\Documents\codex-projects\velocity\temp_sandbox"; Write-Output in_sandbox` | 0 | 1738ms | 100 | Passed |
| `Get-ChildItem -Filter "*.json" -ErrorAction SilentlyContinue | Select-Object -First 1 | ForEach-Object { $_.Name }` | 0 | 838ms | 100 | Passed |
| `Write-Output 1; Write-Output 2` | 0 | 1512ms | 100 | Passed |
| `if (!(Test-Path -LiteralPath "non_existent")) { Write-Output fallback }` | 0 | 3697ms | 100 | Passed |
| `echo "Fill 16"` | 0 | 1605ms | 100 | Passed |
| `echo "Fill 17"` | 0 | 1158ms | 100 | Passed |
| `echo "Fill 18"` | 0 | 2114ms | 100 | Passed |
| `echo "Fill 19"` | 0 | 883ms | 100 | Passed |

**Conclusion:** Engine is robust.
