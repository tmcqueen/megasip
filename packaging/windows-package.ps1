# Package an already-built target\release\megasip.exe as MegaSIP-<version>-windows-x64:
# windeployqt gathers Qt, the Visual C++ runtime goes in app-local, and the folder is zipped.
#
#   packaging\windows-package.ps1 <source dir> <output dir>
#
# Needs windeployqt on PATH (install-qt-action puts it there). Writes name=<package name>
# to $GITHUB_OUTPUT when run in GitHub Actions. Like linux-package.sh, this may come from a
# newer commit than the source, so that an older tag can be packaged again.
param(
    [Parameter(Mandatory)] [string] $Source,
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = "Stop"

$Source = (Resolve-Path $Source).Path
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path

$version = (Select-String -Path "$Source\Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "MegaSIP-$version-windows-x64"
$dist = "$Out\$name"
New-Item -ItemType Directory -Force $dist | Out-Null
Copy-Item "$Source\target\release\megasip.exe" $dist
windeployqt --release --qmldir "$Source\crates\megasip-qt\qml" --no-translations `
    --no-system-d3d-compiler --no-opengl-sw "$dist\megasip.exe"
if ($LASTEXITCODE -ne 0) { throw "windeployqt failed" }
# The Visual C++ runtime, app-local, so it runs where it isn't installed.
foreach ($dll in "vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll", "msvcp140_1.dll") {
    Copy-Item "$env:SystemRoot\System32\$dll" $dist
}
Copy-Item "$Source\LICENSE" "$dist\LICENSE.txt"
Copy-Item "$Source\crates\megasip-qt\icons\README.md" "$dist\ICONS-LICENSE.md"
Compress-Archive -Force -Path $dist -DestinationPath "$Out\$name.zip"
if ($env:GITHUB_OUTPUT) { "name=$name" >> $env:GITHUB_OUTPUT }
Get-ChildItem $Out
