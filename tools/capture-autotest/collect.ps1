param(
    [Parameter(Mandatory=$true)][string]$RunDirectory,
    [string]$Drive = 'F:\'
)
$ErrorActionPreference='Stop'
$root=$PSScriptRoot
$repo=Split-Path (Split-Path $root -Parent) -Parent
$run=(Resolve-Path -LiteralPath $RunDirectory).Path
$allowed=(Resolve-Path -LiteralPath (Join-Path $repo 'CY4500_logs')).Path.TrimEnd('\')+'\'
if (!$run.StartsWith($allowed,[StringComparison]::OrdinalIgnoreCase)) { throw 'Select the same-run folder under CY4500_logs.' }
if (!(Test-Path -LiteralPath (Join-Path $run 'CY4500.csv'))) { throw 'CY4500.csv is missing from the run folder.' }
if (!(Test-Path -LiteralPath (Join-Path $run 'capture-test-plan.json'))) { throw 'The run was not created by this test sweep wrapper.' }
if (!(Test-Path -LiteralPath (Join-Path $Drive 'CAPFLASH.BIN'))) { throw 'CAPFLASH.BIN is missing. Enter USB export mode in the capture-autotest build.' }
$files=@(Get-Item -LiteralPath (Join-Path $Drive 'CAPFLASH.BIN'))+@(Get-ChildItem -LiteralPath $Drive -File -Filter '*.BMP')
foreach($f in $files) {
    if (Test-Path -LiteralPath (Join-Path $run $f.Name)) { throw "File already exists: $($f.Name); do not mix exports." }
}
foreach($f in $files) { Copy-Item -LiteralPath $f.FullName -Destination (Join-Path $run $f.Name) }
$python=Join-Path $repo 'target/capture-python/python.exe'
if (!(Test-Path -LiteralPath $python)) { $python='python' }
$mode=(Get-Content -LiteralPath (Join-Path $run 'capture-test-plan.json') -Raw -Encoding utf8 | ConvertFrom-Json).mode
& $python (Join-Path $root 'analyze.py') (Join-Path $run 'CAPFLASH.BIN') --cy (Join-Path $run 'CY4500.csv') --expected-mode $mode --out (Join-Path $run 'capture-comparison.json')
if ($LASTEXITCODE -ne 0) { throw 'The analysis requires review. Exported evidence is retained; see the analyzer output and any generated capture-comparison.json. Do not repeat collection into the same folder.' }
