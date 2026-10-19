# Build the one-click Claude Desktop extension (.mcpb) for Voltaris on Windows.
#
# Usage: powershell -ExecutionPolicy Bypass -File packaging\build-mcpb.ps1 [-Version X.Y.Z] [-Binary PATH] [-Out DIR]
# Defaults: version from Cargo.toml, binary target\release\voltaris.exe, output dist\.
# Needs Node.js (npx) for the official @anthropic-ai/mcpb packer.
param(
    [string]$Version = "",
    [string]$Binary = "",
    [string]$Out = ""
)
$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
if (-not $Binary) { $Binary = Join-Path $RepoRoot "target\release\voltaris.exe" }
if (-not $Out) { $Out = Join-Path $RepoRoot "dist" }
if (-not $Version) {
    $Version = (Select-String -Path (Join-Path $RepoRoot "Cargo.toml") -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
}
if (-not (Test-Path $Binary)) { throw "binary not found: $Binary (build with: cargo build --release -p voltaris)" }
$Mcpb = "@anthropic-ai/mcpb@2.1.2"

$Stage = Join-Path ([System.IO.Path]::GetTempPath()) ("kcp-mcpb-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path (Join-Path $Stage "server"), $Out | Out-Null
try {
    Copy-Item $Binary (Join-Path $Stage "server\voltaris.exe")
    Copy-Item (Join-Path $RepoRoot "packaging\mcpb\icon.png") (Join-Path $Stage "icon.png")
    Copy-Item (Join-Path $RepoRoot "LICENSE") (Join-Path $Stage "LICENSE")
    $Manifest = Get-Content -Raw (Join-Path $RepoRoot "packaging\mcpb\manifest.template.json")
    $Manifest = $Manifest.Replace("{{VERSION}}", $Version).Replace("{{BIN}}", "voltaris.exe").Replace("{{PLATFORM}}", "win32")
    [System.IO.File]::WriteAllText((Join-Path $Stage "manifest.json"), $Manifest, (New-Object System.Text.UTF8Encoding $false))

    npx --yes $Mcpb validate (Join-Path $Stage "manifest.json")
    if ($LASTEXITCODE -ne 0) { throw "manifest validation failed" }
    $Bundle = Join-Path $Out "voltaris-$Version-win32.mcpb"
    npx --yes $Mcpb pack $Stage $Bundle
    if ($LASTEXITCODE -ne 0) { throw "mcpb pack failed" }
    Write-Host $Bundle
    (Get-FileHash -Algorithm SHA256 $Bundle).Hash
}
finally {
    Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
}
