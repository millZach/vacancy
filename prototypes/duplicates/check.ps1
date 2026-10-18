# Invoke this under Invoke-HeavyJob.ps1, with no nested wrapper. Typecheck, build, then the Edge proof.
$ErrorActionPreference = 'Stop'
$prototypeRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
Set-Location -LiteralPath $prototypeRoot
& 'C:\Program Files\nodejs\node.exe' node_modules/typescript/bin/tsc --noEmit -p prototypes/duplicates/tsconfig.json
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& 'C:\Program Files\nodejs\node.exe' node_modules/vite/bin/vite.js build --config prototypes/duplicates/vite.config.ts
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& 'C:\Program Files\nodejs\node.exe' prototypes/duplicates/proof.mjs
exit $LASTEXITCODE
