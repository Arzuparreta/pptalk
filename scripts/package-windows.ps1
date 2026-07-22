param([string]$Version = "dev")
$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$BuildDir = Join-Path $RepoRoot "build/package-windows"
$StageDir = Join-Path $BuildDir "pptalk"

cargo build --locked --release --manifest-path "$RepoRoot/Cargo.toml" -p pptalk-cli -p pptalk-node
cmake -S "$RepoRoot/apps/desktop" -B "$BuildDir/desktop" -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build "$BuildDir/desktop"
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
Copy-Item "$BuildDir/desktop/pptalk-desktop.exe" $StageDir
Copy-Item "$RepoRoot/target/release/pptalk-cli.exe" $StageDir
Copy-Item "$RepoRoot/target/release/pptalk-node.exe" $StageDir
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
$Artifact = Join-Path "$RepoRoot/build" "pptalk-$Version-windows-x86_64.zip"
Compress-Archive -Path "$StageDir/*" -DestinationPath $Artifact -Force
Write-Output $Artifact
