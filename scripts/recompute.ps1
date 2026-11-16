param(
    [ValidateRange(1, 32)]
    [int]$Threads = 16,
    [string]$ToolsDir = (Join-Path (Split-Path $PSScriptRoot -Parent) 'build\solver'),
    [string]$AssetDir = (Join-Path (Split-Path $PSScriptRoot -Parent) 'release-assets\v1.0.0')
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
$toolPath = (Resolve-Path -LiteralPath $ToolsDir).Path
$required = @('retro5.exe', 'retro_dtm_exact.exe', 'verify.exe', 'table_storage.py', 'compress_dtm.py')
foreach ($name in $required) {
    if (-not (Test-Path -LiteralPath (Join-Path $toolPath $name) -PathType Leaf)) {
        throw "Missing $name in $toolPath. Download and extract the Windows compiled-tools GitHub Actions artifact to build\solver."
    }
}
$existing = Get-ChildItem -LiteralPath $toolPath -File | Where-Object {
    $_.Name -match '^(wdl_s\d+\.(bin|wdlz)|dtm_exact_s\d+\.(dtmz|oridx|orpre|packed))$'
}
if ($existing) {
    throw "Table files already exist in $toolPath. Choose a fresh ToolsDir so this run cannot overwrite existing data."
}
if (Test-Path -LiteralPath $AssetDir) {
    $existingArchives = Get-ChildItem -LiteralPath $AssetDir -File -ErrorAction SilentlyContinue
    if ($existingArchives) {
        throw "Release asset folder is not empty: $AssetDir. Choose a fresh AssetDir."
    }
}

function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE" }
}

Push-Location $toolPath
try {
    Write-Host 'Solving WDL for every layer...'
    Invoke-Checked '.\retro5.exe' @('--sheep', '15', '--lose', '4', '--time', '18000', '--threads', "$Threads", '--save')

    Write-Host 'Running the complete WDL successor verifier...'
    Invoke-Checked '.\verify.exe' @('--threads', "$Threads")

    Write-Host 'Solving exact DTM and checking every layer...'
    Invoke-Checked '.\retro_dtm_exact.exe' @('--from', '4', '--to', '15', '--threads', "$Threads")

    Write-Host 'Losslessly packing WDL...'
    Invoke-Checked 'python' @('table_storage.py', 'pack-wdl', '--threads', "$Threads")

    Write-Host 'Verifying packed WDL, DTMZ, and ORIDX data...'
    Invoke-Checked 'python' @('table_storage.py', 'verify')

    Write-Host 'Removing only the raw WDL intermediates created by this run...'
    Get-ChildItem -LiteralPath . -Filter 'wdl_s*.bin' -File | ForEach-Object { Remove-Item -LiteralPath $_.FullName }

    Write-Host 'Verifying the compressed database without raw intermediates...'
    Invoke-Checked 'python' @('table_storage.py', 'verify')
}
finally {
    Pop-Location
}

New-Item -ItemType Directory -Path $AssetDir -Force | Out-Null
$assetPath = (Resolve-Path -LiteralPath $AssetDir).Path
Invoke-Checked 'python' @((Join-Path $PSScriptRoot 'package_release_data.py'), '--data-dir', $toolPath, '--output-dir', $assetPath)
Write-Host "Full solve, verification, and release packaging completed in $assetPath"
