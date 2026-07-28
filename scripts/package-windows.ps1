param([string]$Version = "dev")
$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$BuildDir = Join-Path $RepoRoot "build/package-windows"
$StageDir = Join-Path $BuildDir "pptalk"

cargo build --locked --release --manifest-path "$RepoRoot/Cargo.toml" -p pptalk-cli
cmake -S "$RepoRoot/apps/desktop" -B "$BuildDir/desktop" -G Ninja `
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_COMPILER=cl
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
$PluginDir = Join-Path $StageDir "lib/gstreamer-1.0"
function Add-GStreamerElement {
    param([string]$Element, [bool]$Required = $true)
    $Output = & gst-inspect-1.0.exe $Element 2>$null
    $Match = $Output | Select-String -Pattern '^\s*Filename\s+(.+)$' | Select-Object -First 1
    if (-not $Match) {
        if ($Required) { throw "Required GStreamer element is unavailable: $Element" }
        return $false
    }
    $Plugin = $Match.Matches[0].Groups[1].Value.Trim()
    if (-not (Test-Path $Plugin)) {
        if ($Required) { throw "GStreamer plugin does not exist: $Plugin" }
        return $false
    }
    Copy-Item $Plugin $PluginDir -Force
    return $true
}
$RequiredElements = @(
    "appsrc", "appsink", "queue", "audioconvert", "audioresample",
    "opusenc", "opusdec", "rtpopuspay", "rtpopusdepay", "rtpjitterbuffer",
    "volume", "videoconvert", "videoscale", "videorate", "h264parse",
    "rtph264pay", "rtph264depay", "decodebin", "autoaudiosink", "autovideosink"
)
foreach ($Element in $RequiredElements) {
    Add-GStreamerElement -Element $Element | Out-Null
}
function Add-AtLeastOneGStreamerElement {
    param([string]$Description, [string[]]$Elements)
    $Found = $false
    foreach ($Element in $Elements) {
        if (Add-GStreamerElement -Element $Element -Required $false) { $Found = $true }
    }
    if (-not $Found) { throw "No supported GStreamer $Description plugin is available" }
}
Add-AtLeastOneGStreamerElement "audio capture" @("wasapisrc")
Add-AtLeastOneGStreamerElement "camera capture" @("mfvideosrc", "d3d11videosrc")
Add-AtLeastOneGStreamerElement "screen capture" @("d3d11screencapturesrc")
Add-AtLeastOneGStreamerElement "H.264 encoder" @("openh264enc", "x264enc")
Add-AtLeastOneGStreamerElement "H.264 decoder" @("openh264dec", "avdec_h264")
foreach ($Element in @("wasapisink", "d3d11videosink")) {
    Add-GStreamerElement -Element $Element -Required $false | Out-Null
}
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
$ReleaseDate = Get-Date -Format "yyyy-MM-dd"
(Get-Content $PackageXml -Raw).Replace("@RELEASE_DATE@", $ReleaseDate) | Set-Content $PackageXml
$ConfigXml = Join-Path $IfwRoot "config/config.xml"
(Get-Content $ConfigXml -Raw).Replace("@VERSION@", $Version.TrimStart("v")) | Set-Content $ConfigXml
$BinaryCreator = (Get-Command binarycreator.exe -ErrorAction Stop).Source
$Artifact = Join-Path "$RepoRoot/build" "pptalk-$Version-windows-x86_64.exe"
& $BinaryCreator -c "$IfwRoot/config/config.xml" -p "$IfwRoot/packages" $Artifact
Write-Output $Artifact
