Param(
  [Parameter(Mandatory = $true)]
  [string]$Name,
  [Parameter(Mandatory = $true)]
  [string]$Version,
  [Parameter(Mandatory = $true)]
  [string]$SourceDir,
  [Parameter(Mandatory = $true)]
  [string]$ExeName,
  [Parameter(Mandatory = $true)]
  [string]$UpgradeCode,
  [Parameter(Mandatory = $true)]
  [string]$OutputDir,
  [string]$Manufacturer = "OpenWhisperAI"
)

$ErrorActionPreference = "Stop"

if (-not (Get-Command candle.exe -ErrorAction SilentlyContinue)) {
  throw "WiX candle.exe not found on PATH"
}
if (-not (Get-Command light.exe -ErrorAction SilentlyContinue)) {
  throw "WiX light.exe not found on PATH"
}

$ResolvedSource = (Resolve-Path $SourceDir).Path

if (-not (Test-Path $OutputDir)) {
  New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
}

$ResolvedOutput = (Resolve-Path $OutputDir).Path
$WxsPath = Join-Path $PSScriptRoot "windows\installer.wxs"

if (-not (Test-Path $WxsPath)) {
  throw "Missing WiX template: $WxsPath"
}

if (-not (Test-Path (Join-Path $ResolvedSource $ExeName))) {
  throw "Executable not found: $(Join-Path $ResolvedSource $ExeName)"
}

New-Item -ItemType Directory -Path $ResolvedOutput -Force | Out-Null

$BuildDir = Join-Path $ResolvedOutput "wix-build"
New-Item -ItemType Directory -Path $BuildDir -Force | Out-Null

$WixObj = Join-Path $BuildDir "installer.wixobj"
$MsiName = "${Name}-${Version}-x64.msi"
$MsiPath = Join-Path $ResolvedOutput $MsiName

& candle.exe -nologo `
  -dSourceDir=$ResolvedSource `
  -dExeName=$ExeName `
  -dProductName=$Name `
  -dProductVersion=$Version `
  -dManufacturer=$Manufacturer `
  -dUpgradeCode=$UpgradeCode `
  -out $WixObj `
  $WxsPath

& light.exe -nologo -out $MsiPath $WixObj

Write-Host "Created $MsiPath"
