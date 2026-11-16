<#
.SYNOPSIS
  Builds and tests the TIPS Tracker Toolbox plugin inside Toolbox's own CMake tree.
.EXAMPLE
  .\build.ps1 -Target tips_tests -Run
  .\build.ps1 -Target TipsTracker -Dist -ApiBase https://example.net
#>
[CmdletBinding()]
param(
    [ValidateSet('tips_tests', 'TipsTracker')][string]$Target = 'tips_tests',
    [switch]$Run,
    [switch]$Dist,
    [string]$Config = 'RelWithDebInfo',
    [string]$ApiBase = 'https://tips-run.com',
    [string]$ToolboxDir = '',
    [switch]$Reconfigure
)
# Native tools (git, cmake) write progress to stderr; check exit codes instead of treating that as an error.
$ErrorActionPreference = 'Continue'
$root = $PSScriptRoot
$repo = Split-Path $root -Parent
if (-not $ToolboxDir) { $ToolboxDir = Join-Path $repo 'reference_material\GWToolboxpp' }
if (-not (Test-Path $ToolboxDir)) { throw "Toolbox source not found at $ToolboxDir (see toolbox-plugin/README.md)" }

# 1. The Toolbox checkout must be the pinned release.
$pin = Get-Content (Join-Path $root 'TOOLBOX_VERSION')
$head = (git -C $ToolboxDir rev-parse --short=9 HEAD | Out-String).Trim()
if (-not $head.StartsWith($pin[1].Trim())) {
    throw "Toolbox at $ToolboxDir is $head but the plugin is pinned to $($pin[0]) ($($pin[1])). Check out the pinned release."
}

# 2. Visual Studio's CMake (the VS 2026 generator, as Toolbox's own preset uses).
$vs = @('Community', 'Professional', 'Enterprise') |
    ForEach-Object { "C:\Program Files\Microsoft Visual Studio\18\$_" } |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $vs) { throw 'Visual Studio 18 (2026) with the C++ desktop workload is required (Toolbox 8.34 builds with it).' }
$cmake = Join-Path $vs 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'

# 3. vcpkg (the tool only; Toolbox's vcpkg-configuration.json pins the package baseline).
$vcpkg = Join-Path $root '.tools\vcpkg'
if (-not (Test-Path (Join-Path $vcpkg 'vcpkg.exe'))) {
    if (-not (Test-Path (Join-Path $vcpkg 'bootstrap-vcpkg.bat'))) {
        git clone --depth 1 https://github.com/microsoft/vcpkg $vcpkg
        if ($LASTEXITCODE -ne 0) { throw 'git clone vcpkg failed' }
    }
    & (Join-Path $vcpkg 'bootstrap-vcpkg.bat') -disableMetrics
    if ($LASTEXITCODE -ne 0) { throw 'bootstrap vcpkg failed' }
}
$env:VCPKG_ROOT = $vcpkg

# 4. The junction that lets Toolbox's add_tb_plugin() find our sources.
$junction = Join-Path $ToolboxDir 'plugins\TipsTracker'
if (-not (Test-Path $junction)) {
    New-Item -ItemType Junction -Path $junction -Target (Join-Path $root 'plugin') | Out-Null
}

# 5. Configure (once) and build.
$buildDir = Join-Path $root 'build'
$cmRoot = $root.Replace('\', '/')   # CMake wants forward slashes
# Configure every time: it takes seconds, and it is what picks up newly added source files.
if ($true) {
    & $cmake -S $ToolboxDir -B $buildDir -G 'Visual Studio 18 2026' -A Win32 `
        "-DCMAKE_TOOLCHAIN_FILE=$vcpkg\scripts\buildsystems\vcpkg.cmake" `
        "-DVCPKG_OVERLAY_TRIPLETS=$ToolboxDir\vcpkg-overlays\triplets" `
        '-DVCPKG_TARGET_TRIPLET=x86-windows-mixed' `
        '-DGWTOOLBOX_BUILD_EXAMPLE_PLUGIN=OFF' `
        "-DCMAKE_PROJECT_gwtoolbox_INCLUDE=$cmRoot/cmake/tips.cmake" `
        "-DTIPS_ROOT=$cmRoot" "-DTIPS_API_BASE=$ApiBase"
    if ($LASTEXITCODE -ne 0) { throw 'configure failed' }
}
& $cmake --build $buildDir --config $Config --target $Target -- /m /nologo /v:m
if ($LASTEXITCODE -ne 0) { throw "build of $Target failed" }

# 6. Run the tests / collect the DLL.
$bin = Join-Path $ToolboxDir 'bin'
if ($Run -and $Target -eq 'tips_tests') {
    $exe = Get-ChildItem $bin -Recurse -Filter tips_tests.exe | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    & $exe.FullName
    if ($LASTEXITCODE -ne 0) { throw 'tests failed' }
}
if ($Dist -and $Target -eq 'TipsTracker') {
    $dll = Get-ChildItem $bin -Recurse -Filter TipsTracker.dll | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    $out = Join-Path $root 'dist'
    New-Item -ItemType Directory -Force $out | Out-Null
    Copy-Item $dll.FullName $out -Force
    Write-Host "dist: $out\TipsTracker.dll"
}
