# Copies blurman.exe and the files its window needs beside it (the Windows App Runtime, which
# build.rs stages next to the exe) into one folder: a copy you can move anywhere, or the Store
# package's layout.
#
#   ./packaging/runtime-files.ps1 -Exe target/release/blurman.exe -Out dist/Blurman
#
# -NoIndex leaves out the runtime's resources.pri, for a package whose own resource index takes in
# the runtime's other .pri files instead.

param(
    [Parameter(Mandatory)] [string] $Exe,
    [Parameter(Mandatory)] [string] $Out,
    [switch] $NoIndex
)

$ErrorActionPreference = "Stop"
$from = Split-Path (Resolve-Path $Exe)
New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item $Exe (Join-Path $Out "blurman.exe")
# The runtime's DLLs and resource indexes. A Rust build puts no DLLs of its own here.
Get-ChildItem $from -File |
    Where-Object { $_.Extension -in ".dll", ".pri" } |
    Where-Object { -not ($NoIndex -and $_.Name -eq "resources.pri") } |
    Copy-Item -Destination $Out
# Its folders: Microsoft.UI.Xaml, and one per language. The rest are Cargo's own.
Get-ChildItem $from -Directory |
    Where-Object { $_.Name -notin "build", "deps", "examples", "incremental", ".fingerprint" } |
    Copy-Item -Destination $Out -Recurse
if (-not (Test-Path (Join-Path $Out "Microsoft.WindowsAppRuntime.dll"))) {
    throw "The Windows App Runtime is not next to $Exe. Build Blurman first: build.rs stages it."
}
