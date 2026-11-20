# Render Blurman's icon from the SVGs beside this script: the app icon (assets\blurman.ico and
# the icon-*.png files) and the tray's two icons (assets\tray.ico, assets\tray-paused.ico).
# Edge draws each size headless; run with Windows PowerShell (powershell.exe) for System.Drawing.
param([string]$Edge = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe")
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
$here = $PSScriptRoot
$assets = Split-Path $here
$work = Join-Path ([IO.Path]::GetTempPath()) "blurman-icon-render"
New-Item -ItemType Directory -Force $work | Out-Null
$profileDir = Join-Path $work "edge-profile"

# One SVG at one size, as a PNG with a transparent background. Edge renders a larger window and
# the icon is cropped from its top-left corner (headless windows have a minimum size).
function Render([string]$svg, [int]$size) {
    $html = Join-Path $work "render.html"
    $src = ([Uri](Join-Path $here $svg)).AbsoluteUri
    "<!doctype html><html><body style='margin:0;background:transparent'><img src='$src' width='$size' height='$size' style='display:block'></body></html>" |
        Set-Content $html -Encoding utf8
    $shot = Join-Path $work "shot.png"
    if (Test-Path $shot) { Remove-Item $shot }
    $arguments = @("--headless=new", "--disable-gpu", "--hide-scrollbars", "--force-device-scale-factor=1",
        "--default-background-color=00000000", "--user-data-dir=`"$profileDir`"", "--window-size=600,600",
        "--screenshot=`"$shot`"", ([Uri]$html).AbsoluteUri)
    Start-Process -FilePath $Edge -ArgumentList $arguments -Wait -WindowStyle Hidden
    $deadline = (Get-Date).AddSeconds(20)
    while (-not (Test-Path $shot) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100 }
    $full = [System.Drawing.Bitmap]::FromFile($shot)
    $crop = $full.Clone((New-Object System.Drawing.Rectangle 0, 0, $size, $size), [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $full.Dispose()
    $ms = New-Object IO.MemoryStream
    $crop.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $crop.Dispose()
    return , $ms.ToArray()
}

# An .ico of PNG images, one per size.
function Write-Ico([string]$path, $images) {
    $ms = New-Object IO.MemoryStream
    $w = New-Object IO.BinaryWriter $ms
    $w.Write([UInt16]0); $w.Write([UInt16]1); $w.Write([UInt16]$images.Count)
    $offset = 6 + 16 * $images.Count
    foreach ($image in $images) {
        $side = if ($image.Size -ge 256) { 0 } else { $image.Size }
        $w.Write([byte]$side); $w.Write([byte]$side); $w.Write([byte]0); $w.Write([byte]0)
        $w.Write([UInt16]1); $w.Write([UInt16]32)
        $w.Write([UInt32]$image.Png.Length); $w.Write([UInt32]$offset)
        $offset += $image.Png.Length
    }
    foreach ($image in $images) { $w.Write($image.Png) }
    [IO.File]::WriteAllBytes($path, $ms.ToArray())
    "wrote $path ($($images.Count) sizes)"
}

function Set-Of([string]$bigSvg, [string]$smallSvg, [int[]]$sizes) {
    foreach ($size in $sizes) {
        $svg = if ($size -lt 32) { $smallSvg } else { $bigSvg }
        [pscustomobject]@{ Size = $size; Png = (Render $svg $size) }
    }
}

$app = @(Set-Of "blurman.svg" "blurman-small.svg" @(16, 20, 24, 32, 40, 48, 64, 128, 256))
Write-Ico (Join-Path $assets "blurman.ico") $app
foreach ($size in 32, 64, 256) {
    [IO.File]::WriteAllBytes((Join-Path $assets "icon-$size.png"), ($app | Where-Object Size -eq $size).Png)
}
$tray = @(Set-Of "blurman-small.svg" "blurman-small.svg" @(16, 20, 24, 32))
Write-Ico (Join-Path $assets "tray.ico") $tray
# The window's title bar shows the icon at 16 px: the small variant, at 32 for high DPI.
[IO.File]::WriteAllBytes((Join-Path $assets "icon-small-32.png"), ($tray | Where-Object Size -eq 32).Png)
Write-Ico (Join-Path $assets "tray-paused.ico") @(Set-Of "blurman-paused.svg" "blurman-paused.svg" @(16, 20, 24, 32))
