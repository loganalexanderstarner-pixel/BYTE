<#
Copies the four MSVC runtime DLLs that llama-server.exe needs into
vendor/windows-runtime/, ready for the installer to bundle.

Why this exists: MSVCP140, VCRUNTIME140, VCRUNTIME140_1 and VCOMP140 do not ship
with Windows. On the test machine Windows was installed 2026-05-15 and the files
are dated 2026-05-27, with a VC++ redistributable registered. A clean machine
lacks them, and BYTE then fails to start its engine with a missing-DLL error.
See docs/WINDOWS-PACKAGING.md.

They are Microsoft's redistributable binaries, so they are copied at build time
and never committed (vendor/windows-runtime/ is gitignored).

Search order, most to least specific, because the redistributable-supplied copy
is the one Microsoft licenses for redistribution:
  1. Visual Studio's own redist folder  (...\VC\Redist\MSVC\<ver>\x64\)
  2. System32, where the VC++ redistributable installs them
#>
param([switch]$Cuda, [ValidateSet('x64', 'arm64')][string]$Arch = 'x64')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root 'vendor\windows-runtime'
New-Item -ItemType Directory -Force -Path $dest | Out-Null

$need = @('MSVCP140.dll', 'VCRUNTIME140.dll', 'VCRUNTIME140_1.dll', 'VCOMP140.DLL')
# VCRUNTIME140_1 belongs to x64 exception handling. A native ARM64 program never imports it, and the
# copy in the ARM64 redistributable folder is an x64 file (machine type 8664, found by checking what
# the installer carried), so it is not shipped there.
if ($Arch -eq 'arm64') { $need = $need | Where-Object { $_ -ne 'VCRUNTIME140_1.dll' } }

$searchDirs = @()
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (Test-Path $vswhere) {
  $vs = & $vswhere -products * -latest -property installationPath
  if ($vs) {
    $redist = Join-Path $vs 'VC\Redist\MSVC'
    if (Test-Path $redist) {
      Get-ChildItem $redist -Directory | Sort-Object Name -Descending | ForEach-Object {
        $searchDirs += Join-Path $_.FullName "$Arch\Microsoft.VC143.CRT"
        $searchDirs += Join-Path $_.FullName "$Arch\Microsoft.VC143.OpenMP"
      }
    }
  }
}
# System32 holds the DLLs of the machine's own architecture. That is only the right copy when
# it matches the one being built for: an ARM64 build made on an x64 runner must NOT take the
# x64 files from here (it would ship an installer whose engine cannot load them).
if ($Arch -eq 'x64') { $searchDirs += (Join-Path $env:SystemRoot 'System32') }

$missing = @()
foreach ($name in $need) {
  $found = $null
  foreach ($d in $searchDirs) {
    $p = Join-Path $d $name
    if (Test-Path $p) { $found = $p; break }
  }
  if ($found) {
    Copy-Item $found (Join-Path $dest $name) -Force
    $v = (Get-Item $found).VersionInfo.FileVersion
    Write-Host ("  {0,-22} {1}   <- {2}" -f $name, $v, (Split-Path $found))
  } else {
    $missing += $name
  }
}

if ($missing.Count -gt 0) {
  # Fail loudly. Shipping an installer without these produces an app that
  # installs fine and then cannot start its engine, which is far worse than a
  # failed build.
  Write-Error ("Could not find: " + ($missing -join ', ') +
    ". Install the Visual C++ redistributable (https://aka.ms/vs/17/release/vc_redist.x64.exe) or Visual Studio Build Tools, then re-run.")
  exit 1
}

# cuBLAS for the CUDA engine. llama-server links cublas64_13.dll dynamically,
# so without it the engine does not start AT ALL on a machine lacking the CUDA
# toolkit -- including machines with no NVIDIA card, where it should still be
# able to start and fall back. cuBLAS is on NVIDIA's redistributable list.
if ($Cuda) {
  $cudaRoot = $env:CUDA_PATH
  if (-not $cudaRoot) { Write-Error "-Cuda given but CUDA_PATH is not set."; exit 1 }
  foreach ($name in @('cublas64_13.dll', 'cublasLt64_13.dll')) {
    # CUDA 13 keeps the x64 binaries in bin\x64; earlier layouts used bin\.
    $p = @((Join-Path $cudaRoot "bin\x64\$name"), (Join-Path $cudaRoot "bin\$name")) |
         Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $p) { Write-Error "Could not find $name under $cudaRoot"; exit 1 }
    Copy-Item $p (Join-Path $dest $name) -Force
    Write-Host ("  {0,-22} {1} MB   <- {2}" -f $name, [math]::Round((Get-Item $p).Length/1MB), (Split-Path $p))
  }
}
Write-Host "Runtime DLLs staged in $dest"
