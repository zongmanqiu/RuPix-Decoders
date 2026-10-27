# RuPix toolchain bootstrap.
#
# Dot-source this from build scripts, or run it standalone to print the report:
#   . .\toolchain.ps1 ; Initialize-RuPixToolchain ; Write-ToolchainReport
#
# WHY: "it builds on my machine" does not count (RuPix-Decoder/AGENTS.md / the decoder
# spec section 6 rule 9). This script pins the sparse facts of THIS box so every build
# log carries the same evidence:
#   1. Rust toolchain    - rustup has no default configured here -> set RUSTUP_TOOLCHAIN.
#   2. MSVC via vswhere  - locate vcvars64.bat for form-B (CMake/C) sub-builds.
#   3. NASM / CMake      - export NASM_PREFIX / CMAKE_GENERATOR; report versions.
#   4. CC=gcc guard      - this box HAS gcc 16 in PATH; MSVC/MinGW mixed linking
#                          re-runs K06 (CRT duplicate / link conflict). Refuse early.
#
# ASCII-only on purpose: PowerShell 5.1 reads non-BOM UTF-8 files as ANSI (K27).

$script:RuPixToolchain = [ordered]@{
    RustToolchain = ""
    VcVars64      = $null
    Nasm          = $null
    Cmake         = $null
    Meson         = $null
    Cl            = $null
}

function Initialize-RuPixToolchain {
    param(
        # $true when at least one form-B (CMakeLists.txt) decoder is present:
        # then vcvars64 / nasm / cmake are REQUIRED (fail fast, never degrade to MinGW).
        [switch]$RequireNative
    )

    # --- 1. Rust toolchain: rustup has no default on this box ---
    if (-not $env:RUSTUP_TOOLCHAIN) {
        $env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-msvc"
    }
    $script:RuPixToolchain.RustToolchain = $env:RUSTUP_TOOLCHAIN

    # --- 2. CC=gcc guard (V4: gcc is in PATH here; mixed linking = K06) ---
    if ($env:CC -and ($env:CC -match "gcc")) {
        Write-Error ("toolchain: CC={0} - this repo links with MSVC only (K06 mixed CRT); unset CC" -f $env:CC)
        exit 1
    }

    # --- 3. vcvars64.bat via vswhere ---
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    $vcvars = $null
    if (Test-Path $vswhere) {
        $vsPath = & $vswhere -latest -products * `
            -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            -property installationPath 2>$null
        if ($vsPath) {
            $candidate = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
            if (Test-Path $candidate) { $vcvars = $candidate }
        }
    }
    $script:RuPixToolchain.VcVars64 = $vcvars

    # --- 4. NASM / CMake / Meson probes (record whatever exists) ---
    $nasm = Get-Command nasm.exe -ErrorAction SilentlyContinue
    $cmake = Get-Command cmake.exe -ErrorAction SilentlyContinue
    $meson = Get-Command meson.exe -ErrorAction SilentlyContinue
    if ($nasm) {
        # libjpeg-turbo's CMake wants NASM_PREFIX pointing at the NASM install dir.
        $env:NASM_PREFIX = (Split-Path $nasm.Source -Parent) + "\"
        $script:RuPixToolchain.Nasm = $nasm.Source
    }
    if ($cmake) {
        # Paired with vcvars64 (cl in PATH): NMake Makefiles is the classic combo.
        $env:CMAKE_GENERATOR = "NMake Makefiles"
        $script:RuPixToolchain.Cmake = $cmake.Source
    }
    if ($meson) { $script:RuPixToolchain.Meson = $meson.Source }

    # --- 5. Form-B required-tool gate: fail loudly, never degrade silently ---
    if ($RequireNative) {
        $missing = @()
        if (-not $vcvars) { $missing += "vcvars64.bat (vswhere)" }
        if (-not $nasm) { $missing += "nasm" }
        if (-not $cmake) { $missing += "cmake" }
        if ($missing.Count -gt 0) {
            Write-Error ("toolchain: form-B decoder present but missing: {0}" -f ($missing -join ", "))
            exit 1
        }
    }
}

# cmd /c "call vcvars64.bat && set" -> import the environment into this session.
function Import-VcVars64 {
    param([string]$VcVarsPath)
    if (-not $VcVarsPath) { Write-Error "toolchain: vcvars64.bat not located"; exit 1 }
    $out = cmd /c ("call `"{0}`" >nul 2>&1 && set" -f $VcVarsPath)
    if ($LASTEXITCODE -ne 0) {
        Write-Error ("toolchain: failed to import vcvars64 ({0})" -f $VcVarsPath)
        exit 1
    }
    foreach ($line in $out) {
        if ($line -match "^([^=]+)=(.*)$") {
            Set-Item -Path ("env:" + $Matches[1]) -Value $Matches[2] -ErrorAction SilentlyContinue
        }
    }
    # cl is only real after the import; capture its version for the report.
    $cl = Get-Command cl.exe -ErrorAction SilentlyContinue
    if ($cl) { $script:RuPixToolchain.Cl = $cl.Source }
}

# Version report -> build log (and optionally a file). Evidence for reproducibility.
function Write-ToolchainReport {
    param([string]$LogPath)
    function _ver($exe, $arg) {
        if (-not $exe) { return "<not found>" }
        try { return ((& $exe $arg 2>&1 | Select-Object -First 1) -join " ").Trim() }
        catch { return "<probe failed>" }
    }
    $t = $script:RuPixToolchain
    $lines = @()
    $lines += ("--- toolchain report ---")
    $lines += ("RUSTUP_TOOLCHAIN : {0}" -f $t.RustToolchain)
    $lines += ("rustc            : {0}" -f (_ver "rustc" "--version"))
    $lines += ("vcvars64         : {0}" -f ($(if ($t.VcVars64) { $t.VcVars64 } else { "<not found>" })))
    $clTxt = if ($t.Cl) { (& $t.Cl 2>&1 | Select-Object -First 1) } else { "<resolved after vcvars64 import; form-B builds only>" }
    $lines += ("cl               : {0}" -f $clTxt)
    $lines += ("nasm             : {0}" -f (_ver $t.Nasm "-v"))
    $lines += ("cmake            : {0}" -f (_ver $t.Cmake "--version"))
    $lines += ("meson            : {0}" -f (_ver $t.Meson "--version"))
    $lines += ("CMAKE_GENERATOR  : {0}" -f $env:CMAKE_GENERATOR)
    $lines += ("NASM_PREFIX      : {0}" -f $env:NASM_PREFIX)
    $lines | ForEach-Object { Write-Host $_ }
    if ($LogPath) {
        $dir = Split-Path $LogPath -Parent
        if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
        $lines | Out-File -FilePath $LogPath -Encoding utf8
    }
}
