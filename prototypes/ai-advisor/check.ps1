# Invoke this under Invoke-HeavyJob.ps1, with no nested wrapper.
$ErrorActionPreference = 'Stop'
$prototypeRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
Set-Location -LiteralPath $prototypeRoot
& 'C:\Program Files\nodejs\node.exe' node_modules/typescript/bin/tsc --noEmit -p prototypes/ai-advisor/tsconfig.json
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& 'C:\Program Files\nodejs\node.exe' node_modules/vite/bin/vite.js build --config prototypes/ai-advisor/vite.config.ts
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& 'C:\Program Files\nodejs\node.exe' prototypes/ai-advisor/proof.mjs
exit $LASTEXITCODE
