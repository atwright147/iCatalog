param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$version = (Select-String -Path crates/icatalog-cli/Cargo.toml -Pattern '^version = "([^"]+)"$').Matches[0].Groups[1].Value
$uiVersion = (Select-String -Path crates/icatalog-ui/Cargo.toml -Pattern '^version = "([^"]+)"$').Matches[0].Groups[1].Value
if ($version -cne $uiVersion -or $version -cnotmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$') {
    throw 'CLI and UI versions must match (X.Y.Z or X.Y.Z-prerelease, without build metadata).'
}
$binaryDir = if ($env:ICATALOG_RELEASE_BINARY_DIR) { $env:ICATALOG_RELEASE_BINARY_DIR } else { "target/$Target/release" }
$outputDir = if ($env:ICATALOG_RELEASE_OUTPUT_DIR) { $env:ICATALOG_RELEASE_OUTPUT_DIR } else { 'target/release-assets' }
$arch = if ($Target -eq 'x86_64-pc-windows-msvc') { 'x64' } else { 'arm64' }
New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
$archive = Join-Path $outputDir "icatalog-$version-windows-$arch.zip"
# Build with the static MSVC runtime (see release.yml), so no redistributable is required.
Compress-Archive -LiteralPath "$binaryDir/icatalog.exe", "$binaryDir/icatalog-ui.exe" -DestinationPath $archive -Force
