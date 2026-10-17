# Run this foreground script inside the shared Invoke-HeavyJob.ps1 wrapper; it owns no persistent server.
param([switch]$SkipInstall)
$prototypeRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
Set-Location -LiteralPath $prototypeRoot
$prototypeOut = Join-Path $prototypeRoot 'shots/advisor-prototype'
$prototypeLogs = Join-Path $prototypeOut 'checks'
New-Item -ItemType Directory -Force -Path $prototypeLogs | Out-Null
$prototypeNode = 'C:\Program Files\nodejs\node.exe'
$prototypeNpm = 'C:\Program Files\nodejs\node_modules\npm\bin\npm-cli.js'
function Invoke-PrototypeStep([string]$Name, [string[]]$StepArgs) {
    $prototypeLog = Join-Path $prototypeLogs "$Name.log"
    & $prototypeNode $prototypeNpm @StepArgs *> $prototypeLog
    $prototypeExit = $LASTEXITCODE
    Get-Content -LiteralPath $prototypeLog -Tail 18
    if ($prototypeExit -ne 0) { exit $prototypeExit }
}
if (-not $SkipInstall) { Invoke-PrototypeStep 'npm-ci' @('ci') }
Invoke-PrototypeStep 'typecheck' @('run','prototype:advisor:check')
Invoke-PrototypeStep 'build' @('run','prototype:advisor:build')
& $prototypeNode tools/advisor-prototype-shots.mjs --serve --out $prototypeOut *> (Join-Path $prototypeLogs 'browser.log')
$prototypeExit = $LASTEXITCODE
Get-Content -LiteralPath (Join-Path $prototypeLogs 'browser.log') -Tail 30
exit $prototypeExit
