param([string]$Version = "dev")
$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$BuildDir = Join-Path $RepoRoot "build/package-windows"
$StageDir = Join-Path $BuildDir "pptalk"

cargo build --locked --release --manifest-path "$RepoRoot/Cargo.toml" -p pptalk-cli
cmake -S "$RepoRoot/apps/desktop" -B "$BuildDir/desktop" -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build "$BuildDir/desktop"
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
Copy-Item "$BuildDir/desktop/pptalk-desktop.exe" $StageDir
Copy-Item "$RepoRoot/target/release/pptalk-cli.exe" $StageDir
Copy-Item "$RepoRoot/README.md" $StageDir
Copy-Item "$RepoRoot/SECURITY.md" $StageDir
Copy-Item "$RepoRoot/docs" "$StageDir/docs" -Recurse
Copy-Item "$RepoRoot/LICENSE" $StageDir
Copy-Item "$RepoRoot/LICENSES" "$StageDir/LICENSES" -Recurse
windeployqt --qmldir "$RepoRoot/apps/desktop/qml" "$StageDir/pptalk-desktop.exe"
$GstRoot = $env:GSTREAMER_1_0_ROOT_MSVC_X86_64
if (-not $GstRoot) { throw "GSTREAMER_1_0_ROOT_MSVC_X86_64 is required" }
Copy-Item "$GstRoot/bin/*.dll" $StageDir
New-Item -ItemType Directory -Force -Path "$StageDir/lib/gstreamer-1.0" | Out-Null
Copy-Item "$GstRoot/lib/gstreamer-1.0/*.dll" "$StageDir/lib/gstreamer-1.0"
New-Item -ItemType Directory -Force -Path "$StageDir/libexec/gstreamer-1.0" | Out-Null
Copy-Item "$GstRoot/libexec/gstreamer-1.0/gst-plugin-scanner.exe" "$StageDir/libexec/gstreamer-1.0"
$IfwRoot = Join-Path $BuildDir "ifw"
if (Test-Path $IfwRoot) { Remove-Item -Recurse -Force $IfwRoot }
$PackageData = Join-Path $IfwRoot "packages/org.pptalk/data"
New-Item -ItemType Directory -Force -Path $PackageData | Out-Null
Copy-Item "$StageDir/*" $PackageData -Recurse -Force
Copy-Item "$RepoRoot/packaging/windows/config" "$IfwRoot/config" -Recurse -Force
New-Item -ItemType Directory -Force -Path "$IfwRoot/packages/org.pptalk/meta" | Out-Null
Copy-Item "$RepoRoot/packaging/windows/packages/org.pptalk/meta/*" "$IfwRoot/packages/org.pptalk/meta" -Force
$PackageXml = Join-Path $IfwRoot "packages/org.pptalk/meta/package.xml"
(Get-Content $PackageXml -Raw).Replace("@VERSION@", $Version.TrimStart("v")) | Set-Content $PackageXml
$ConfigXml = Join-Path $IfwRoot "config/config.xml"
(Get-Content $ConfigXml -Raw).Replace("@VERSION@", $Version.TrimStart("v")) | Set-Content $ConfigXml
$BinaryCreator = (Get-Command binarycreator.exe -ErrorAction Stop).Source
$Artifact = Join-Path "$RepoRoot/build" "pptalk-$Version-windows-x86_64.exe"
& $BinaryCreator -c "$IfwRoot/config/config.xml" -p "$IfwRoot/packages" $Artifact
Write-Output $Artifact
