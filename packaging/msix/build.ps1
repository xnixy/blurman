# Builds the Microsoft Store package: blurman.exe, the manifest, and logos drawn from the app icon.
#
#   ./packaging/msix/build.ps1 -Exe target/release/blurman.exe -Out dist
#
# The Store signs the package when it is published, so this does not sign it. To try it on this
# PC first, turn on Developer Mode and pass -Layout, then register the folder it prints with
# Add-AppxPackage -Register <folder>\AppxManifest.xml.

param(
    [Parameter(Mandatory)] [string] $Exe,
    [string] $Out = "dist",
    # Leave the unpacked folder instead of packing it, for Add-AppxPackage -Register.
    [switch] $Layout
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "../..")
$version = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version = "([^"]+)"' |
    Select-Object -First 1).Matches.Groups[1].Value
# Packages take four numbers, and the Store wants the last one to be 0.
$packageVersion = "$version.0"

if (-not (Test-Path $Out)) {
    New-Item -ItemType Directory $Out | Out-Null
}
$stage = Join-Path (Resolve-Path $Out) "msix-layout"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $stage "Assets") | Out-Null

Copy-Item $Exe (Join-Path $stage "blurman.exe")
$manifest = Get-Content (Join-Path $PSScriptRoot "AppxManifest.xml") -Raw
[IO.File]::WriteAllText((Join-Path $stage "AppxManifest.xml"), $manifest.Replace("{VERSION}", $packageVersion))

# Logos at the sizes Windows asks for, scaled from the 256 px app icon. Wide tiles get the icon
# centered on a clear background.
Add-Type -AssemblyName System.Drawing
$icon = [System.Drawing.Image]::FromFile((Join-Path $root "assets/icon-256.png"))
function Save-Logo([string] $name, [int] $width, [int] $height) {
    $bitmap = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $graphics.Clear([System.Drawing.Color]::Transparent)
    $side = [Math]::Min($width, $height)
    $graphics.DrawImage($icon, [int](($width - $side) / 2), [int](($height - $side) / 2), $side, $side)
    $bitmap.Save((Join-Path $stage "Assets/$name"), [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose()
    $bitmap.Dispose()
}
Save-Logo "StoreLogo.png" 50 50
Save-Logo "Square150x150Logo.png" 150 150
Save-Logo "Wide310x150Logo.png" 310 150
Save-Logo "Square44x44Logo.png" 44 44
foreach ($size in 16, 24, 32, 48, 256) {
    Save-Logo "Square44x44Logo.targetsize-$size.png" $size $size
    # The taskbar and Start show these as they are, without a colored plate.
    Save-Logo "Square44x44Logo.targetsize-${size}_altform-unplated.png" $size $size
}
$icon.Dispose()

if ($Layout) {
    "Unpacked package in $stage"
    return
}

$makeappx = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $makeappx) { throw "makeappx.exe was not found. Install the Windows SDK." }
$package = Join-Path (Resolve-Path $Out) "Blurman_${packageVersion}_x64.msix"
& $makeappx.FullName pack /d $stage /p $package /o
if ($LASTEXITCODE -ne 0) { throw "makeappx failed with exit code $LASTEXITCODE." }
Remove-Item -Recurse -Force $stage
"Built $package"
