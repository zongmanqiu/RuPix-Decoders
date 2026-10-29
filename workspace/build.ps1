# Build all RuPix decoder decoders, run their tests, and publish them.
# Publish dir = ../../.temp/decoder-dist (OUTSIDE this repo - it is source only).
#
# This file is the repo's single entry point: sources live INSIDE the repo
# (decoders/ for code, tools/ for the toolchain bootstrap); artifacts do NOT.
# The host project is optional - see "host sync" below.
# Intermediates do NOT: .cargo/config.toml redirects target-dir to
#   ..\..\.temp\decoder-target, so a git clone of this repo stays clean. The real
#   location is read back from cargo, never hardcoded here.
#
# Conventions (enforced by Cargo.toml + README.md + docs/decoder spec):
#   * decoders/<id>/  directory name == decoder id == [lib] name
#   * [package] version is written explicitly in each decoder's Cargo.toml
#   * ABI version is read from abi/src/lib.rs (IV_ABI_VERSION)
# Output naming: <id>_<ver>.dll  (Windows can't overwrite a loaded DLL,
#   so every published build gets a versioned filename.)
#
# Form A (Rust cdylib): built by the cargo workspace as usual.
# Form B (native C/C++ DLL, marked by decoders/<id>/CMakeLists.txt): CMake sub-build
#   under <target-dir>/cbuild/<id> with vcvars64 + NMake Makefiles; ctest gates publishing.
#   Form-B version comes from CMakeLists' project(<id> VERSION x.y.z).
# Missing build output is a HARD ERROR: a silently missing DLL loses a format at
#   runtime and the fallback chain stops working (never degrade silently).
# Size accounting at the end: per-DLL table + total (informational, no cap).
#
# Host sync (optional, dev convenience): if the sibling host build dir
#   ../.build/decoders exists (i.e. this repo sits next to the RuPix main project),
#   each published DLL is copied there too, after removing the previous DLL with
#   the same id - the host scans a directory and refuses two DLLs claiming one id.
#   A standalone clone simply skips this step; nothing depends on it.
#
# This file is ASCII-only on purpose (no encoding pitfalls, see K27).

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot

# Native commands (cargo / python / cmake) report PROGRESS on stderr. Under
# $ErrorActionPreference = 'Stop', PS 5.1 turns the first such line into a terminating
# NativeCommandError as soon as the output is captured (CI log, `*> file`, piped run) -
# so a *success* message like "Finished `release` profile" aborts the build. Every native
# call below is followed by an explicit $LASTEXITCODE check, so the only thing needed here
# is "stderr stays plain text". Use: $code = Invoke-Native { cargo ... }
function Invoke-Native {
    param([scriptblock]$Block)
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        # Forward the block's stdout to the host instead of letting it become this
        # function's return value: `$code = Invoke-Native {...}` must be an INT.
        # (2026-10-25: without this, the python gates' banner text landed in $code and
        # `$code -ne 0` was true on a *passing* gate.)
        & $Block 2>&1 | ForEach-Object { Write-Host $_ }
        return $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prev
    }
}

# --- 0. toolchain bootstrap (RUSTUP_TOOLCHAIN / NASM_PREFIX / CMAKE_GENERATOR / CC guard) ---
$toolchain = Join-Path $root "tools\toolchain.ps1"
if (-not (Test-Path $toolchain)) { Write-Error "toolchain.ps1 not found: $toolchain"; exit 1 }
. $toolchain
Initialize-RuPixToolchain

# --- 0b. formatting gate: cargo fmt --check ------------------------------------
# AGENTS.md section 7 requires "cargo fmt --check" exit 0, but until 2026-10-17
# NO step actually ran it, so drift sat undetected in abi/src/lib.rs - and fmt
# churn in that file also desyncs the host's byte-for-byte vendor snapshot.
# Check before compiling; --check never rewrites sources (run "cargo fmt" to fix).
Push-Location $root
try {
    $code = Invoke-Native { cargo fmt --check }
    if ($code -ne 0) {
        Write-Host "[ERROR] cargo fmt --check failed - run 'cargo fmt' in $root, then re-sync"
        Write-Host "        the host snapshot main\vendor\rupix-abi if abi\src\lib.rs changed."
        exit 1
    }
} finally {
    Pop-Location
}

# --- 0c. doc + dependency + cap gates (instant, so they run before anything compiles) -----
# AGENTS.md section 7 listed both as delivery checks, but NO step ran them - that is how
# docs/core.md line anchors drifted for several rounds unnoticed (m61, 2026-10-20).
Push-Location $root
try {
    if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
        Write-Host "[ERROR] python is not on PATH - the doc/dependency gates need it"
        exit 1
    }
    foreach ($gate in @("..\tools\check_docs.py", "..\tools\check_history.py", "..\tools\check_abi_refs.py", "..\tools\check_abi_freeze.py", "..\tools\check_workspace_config.py", "..\tools\check_deps.py", "..\tools\check_deps_direction.py", "..\tools\check_caps.py", "..\tools\check_claims.py", "..\tools\check_secrets.py", "..\tools\check_licenses.py")) {
        $code = Invoke-Native { python $gate }
        if ($code -ne 0) {
            Write-Host "[ERROR] $gate failed - fix the docs/history/dependency/direction/cap/claim/secret ledger, do not skip"
            exit 1
        }
    }
    $code = Invoke-Native { python ..\testdata\make_samples.py --check }
    if ($code -ne 0) {
        Write-Host "[ERROR] testdata/make_samples.py --check failed - every claims.tsv suffix must be generated or explicitly skipped"
        exit 1
    }
} finally {
    Pop-Location
}

# --- 0d. lint gate: cargo clippy ------------------------------------------------------
# Also unrun until 2026-10-20: the repo had no clippy step, so a deny-by-default ERROR
# (not_unsafe_ptr_arg_deref on abi kit write_last_error) sat in the contract file and made
# every decoder crate fail to check. -D warnings is NOT optional: without it clippy prints
# warnings and still exits 0, which is a gate in name only (host lesson K69).
Push-Location $root
try {
    $code = Invoke-Native { cargo clippy --all-targets --workspace --release -- -D warnings }
    if ($code -ne 0) {
        Write-Host "[ERROR] cargo clippy failed. Fix it. An #[allow] is only acceptable with"
        Write-Host "        a written reason at the call site plus the rule in docs (decoder spec)."
        exit 1
    }
} finally {
    Pop-Location
}

# --- form-B detection: decoders/<id>/CMakeLists.txt exists -> native (CMake) sub-build ---
$formB = @(Get-ChildItem (Join-Path $root "decoders") -Directory |
    Where-Object { Test-Path (Join-Path $_.FullName "CMakeLists.txt") })
if ($formB.Count -gt 0) {
    # vcvars64 / nasm / cmake become REQUIRED (fail fast, never degrade to MinGW).
    Initialize-RuPixToolchain -RequireNative
    Write-Host ("form-B decoder(s): {0}" -f (($formB | ForEach-Object { $_.Name }) -join ", "))
}

# Intermediates live OUTSIDE this repo: .cargo/config.toml pins target-dir, and
# CARGO_BUILD_TARGET_DIR overrides it. Read the resolved value instead of guessing -
# the release DLLs, form-B cbuild/ and the toolchain report all go there.
$buildRoot = $env:CARGO_BUILD_TARGET_DIR
if (-not $buildRoot) {
    $cfg = Join-Path $root ".cargo\config.toml"
    $m = [regex]::Match((Get-Content $cfg -Raw), '(?m)^target-dir\s*=\s*"([^"]+)"')
    if (-not $m.Success) { Write-Error "no target-dir in $cfg"; exit 1 }
    $buildRoot = $m.Groups[1].Value
}
# config's value is workspace-root-relative ("../../.temp/decoder-target"); normalize it.
$buildRoot = [System.IO.Path]::GetFullPath((Join-Path $root $buildRoot))
Write-Host ("target-dir: {0}" -f $buildRoot)

$target = Join-Path $buildRoot "release"
New-Item -ItemType Directory -Force -Path $target | Out-Null
Write-ToolchainReport (Join-Path $buildRoot "toolchain_report.txt")

Push-Location $root
try {
    # Build each root tier SEPARATELY: one cargo invocation unions the feature
    # sets of common/modern/raw through shared deps (modern grew +448 KB on
    # 2026-10-29). The workspace test run below still covers all of them.
    foreach ($pkg in @("rupix-common", "rupix-modern", "rupix-raw")) {
        $code = Invoke-Native { cargo build --release -p $pkg }
        if ($code -ne 0) { exit $code }
    }
    # Serial, always (AGENTS.md section 7): the decoder decoders keep process-global state
    # (gif/apng SESSIONS is a 2-slot LRU pool). Run in parallel, two tests evict each
    # other's session and a "sequential playback" assertion suddenly sees a rebuilt
    # cursor - a real flake, observed 2026-10-18 as ring_frames()==Some(1).
    $code = Invoke-Native { cargo test --release --workspace -- --test-threads=1 }
    # tests failing should not silently publish broken decoders
    if ($code -ne 0) { exit $code }
} finally {
    Pop-Location
}

# --- bench singles: a SEPARATE cargo workspace (see singles/Cargo.toml) ---
# Singles are NOT published (decoders/ holds the tiers), but their tests are still a
# gate: every tier is assembled from them, so a rotted single lib rots a shipped DLL.
# They must not share one cargo invocation with the tiers: feature unification would
# switch each member's `standalone` back on and the tier DLLs would link duplicate
# `iv_abi_version` symbols (LNK2005, measured 2026-10-25).
$benchRoot = Join-Path $root "singles"
if (Test-Path $benchRoot) {
    Push-Location $benchRoot
    try {
        $code = Invoke-Native { cargo fmt --check }
        if ($code -ne 0) { Write-Host "[ERROR] cargo fmt --check failed in singles"; exit 1 }
        # rupix-doc-legacy is GNU-only (build.rs rejects MSVC) and lives in this
        # workspace for source layout only; exclude it here and build it below.
        $code = Invoke-Native { cargo clippy --all-targets --release --workspace --exclude rupix-doc-legacy -- -D warnings }
        if ($code -ne 0) { Write-Host "[ERROR] cargo clippy failed in singles"; exit 1 }
        $code = Invoke-Native { cargo build --release --workspace --exclude rupix-doc-legacy }
        if ($code -ne 0) { exit $code }
        # Serial, always: same process-global session pools as the tier tests.
        $code = Invoke-Native { cargo test --release --workspace --exclude rupix-doc-legacy -- --test-threads=1 }
        if ($code -ne 0) { exit $code }
        $code = Invoke-Native { cargo clippy --release -p rupix-doc-legacy --target x86_64-pc-windows-gnu -- -D warnings }
        if ($code -ne 0) { Write-Host "[ERROR] cargo clippy failed in singles/doc-legacy"; exit 1 }
        $code = Invoke-Native { cargo build --release -p rupix-doc-legacy --target x86_64-pc-windows-gnu }
        if ($code -ne 0) { exit $code }
        $code = Invoke-Native { cargo test --release -p rupix-doc-legacy --target x86_64-pc-windows-gnu -- --test-threads=1 }
        if ($code -ne 0) { exit $code }
    } finally {
        Pop-Location
    }
}

# --- separate-workspace tiers (pro / classic / doc / doc-legacy / texture) ------------
# Any decoders/<id>/Cargo.toml that declares its own [workspace] is built on its
# own: sharing one cargo invocation would unify features across tiers (measured
# 2026-10-25: common +685 KB, raw +834 KB when pro's image/exr came along).
# Resolve target-dir, target triple and [lib] name from the tier's own manifest
# instead of hardcoding them: doc-legacy builds for x86_64-pc-windows-gnu (so its
# output sits one directory deeper) and its [lib] name is doc_legacy while the
# published id is doc-legacy.
$separate = @(Get-ChildItem (Join-Path $root "decoders") -Directory | Where-Object {
    $toml = Join-Path $_.FullName "Cargo.toml"
    (Test-Path $toml) -and (Select-String -Path $toml -Pattern '^\[workspace\]' -Quiet)
})
foreach ($dir in $separate) {
    $id = $dir.Name
    $tomlPath = Join-Path $dir.FullName "Cargo.toml"
    $manifest = Get-Content $tomlPath -Raw
    # [lib] name is what cargo emits; the published file is <id>.dll.
    $libName = $id
    $lm = [regex]::Match($manifest, '(?ms)^\[lib\][^\[]*?^name\s*=\s*"([^"]+)"')
    if ($lm.Success) { $libName = $lm.Groups[1].Value }

    Push-Location $dir.FullName
    try {
        $code = Invoke-Native { cargo fmt --check }
        if ($code -ne 0) { Write-Host ("[ERROR] cargo fmt --check failed in {0}" -f $id); exit 1 }
        $code = Invoke-Native { cargo clippy --all-targets --release -- -D warnings }
        if ($code -ne 0) { Write-Host ("[ERROR] cargo clippy failed in {0}" -f $id); exit 1 }
        $code = Invoke-Native { cargo build --release }
        if ($code -ne 0) { exit $code }
        # Serial, always: same process-global session pools as the root tier tests.
        $code = Invoke-Native { cargo test --release -- --test-threads=1 }
        if ($code -ne 0) { exit $code }
    } finally {
        Pop-Location
    }

    # Resolve the real target dir through cargo (honors .cargo/config.toml and
    # CARGO_BUILD_TARGET_DIR); fall back to the config value if metadata fails.
    $targetDir = $null
    try {
        $metaJson = & cargo metadata --no-deps --format-version 1 2>$null
        if ($LASTEXITCODE -eq 0 -and $metaJson) {
            $targetDir = ($metaJson | ConvertFrom-Json).target_directory
        }
    } catch { }
    $cfgPath = Join-Path $dir.FullName ".cargo\config.toml"
    if (-not $targetDir) {
        if (-not (Test-Path $cfgPath)) { Write-Host ("[ERROR] no target-dir for {0}" -f $id); exit 1 }
        $m = [regex]::Match((Get-Content $cfgPath -Raw), '(?m)^target-dir\s*=\s*"([^"]+)"')
        if (-not $m.Success) { Write-Host ("[ERROR] no target-dir in {0}" -f $cfgPath); exit 1 }
        $targetDir = [System.IO.Path]::GetFullPath((Join-Path $dir.FullName $m.Groups[1].Value))
    }
    $triple = ""
    if (Test-Path $cfgPath) {
        $tm = [regex]::Match((Get-Content $cfgPath -Raw), '(?m)^target\s*=\s*"([^"]+)"')
        if ($tm.Success) { $triple = $tm.Groups[1].Value }
    }
    $relDir = Join-Path $targetDir "release"
    if ($triple) { $relDir = Join-Path (Join-Path $targetDir $triple) "release" }

    $built = Join-Path $relDir "$libName.dll"
    if (-not (Test-Path $built)) {
        Write-Host ("[ERROR] {0} workspace produced no {1}.dll under {2}" -f $id, $libName, $relDir)
        exit 1
    }
    Copy-Item -Force $built (Join-Path $target "$id.dll")
    $builtPdb = Join-Path $relDir "$libName.pdb"
    if (Test-Path $builtPdb) { Copy-Item -Force $builtPdb (Join-Path $target "$id.pdb") }
    Write-Host ("separate workspace: {0} ({1}.dll)" -f $id, $libName)
}

# --- coverage gate: every decoders/<id> must belong to a build path ------------
# Publish-One hard-errors on a missing DLL, but only after everything is built;
# this makes a tier that was never wired into either workspace fail up front
# (doc/doc-legacy/texture were in that state until 2026-10-29).
$rootToml = Get-Content (Join-Path $root "Cargo.toml") -Raw
$rootMembers = @([regex]::Matches($rootToml, '(?m)^\s*"(decoders/[^"]+)"') | ForEach-Object {
    Split-Path $_.Groups[1].Value -Leaf
})
$builtIds = @($rootMembers) + @($separate | ForEach-Object { $_.Name })
$unbuilt = @(Get-ChildItem (Join-Path $root "decoders") -Directory |
    Where-Object { $builtIds -notcontains $_.Name } | ForEach-Object { $_.Name })
if ($unbuilt.Count -gt 0) {
    Write-Host ("[ERROR] tier(s) not wired into any build path: {0}" -f ($unbuilt -join ", "))
    Write-Host "        add them to workspace/Cargo.toml members or give them their own [workspace]"
    exit 1
}

# --- form-B sub-build (per decoder), stage the DLL into <target-dir>/release ---
foreach ($dir in $formB) {
    $id = $dir.Name
    Import-VcVars64 $script:RuPixToolchain.VcVars64
    $buildDir = Join-Path $buildRoot "cbuild\$id"
    Push-Location $dir.FullName
    try {
        $code = Invoke-Native { cmake -S . -B $buildDir -G $env:CMAKE_GENERATOR }
        if ($code -ne 0) { exit $code }
        $code = Invoke-Native { cmake --build $buildDir }
        if ($code -ne 0) { exit $code }
    } finally {
        Pop-Location
    }
    # ctest gates the publish when the CMake project registered tests (golden + probe).
    if (Test-Path (Join-Path $buildDir "CTestTestfile.cmake")) {
        Push-Location $buildDir
        try {
            $code = Invoke-Native { ctest --output-on-failure }
            if ($code -ne 0) { exit $code }
        } finally {
            Pop-Location
        }
    }
    $built = Join-Path $buildDir "$id.dll"
    if (-not (Test-Path $built)) { Write-Error ("form-B build produced no {0}.dll" -f $id); exit 1 }
    Copy-Item -Force $built (Join-Path $target "$id.dll")
}

# Publish staging lives OUTSIDE the repo, next to the build intermediates:
# the repo is SOURCE ONLY (2026-10-25 decision) - the DLLs reach users through a
# release asset, and build.ps1 also mirrors them into the host .build\decoders.
# Override with $env:RUPIX_DIST_DIR if you want them somewhere else.
# Optional env switches (default = do the thing):
#   RUPIX_NO_HOST_SYNC=1  skip mirroring into ../.build/decoders
#   RUPIX_NO_SYMBOLS=1    skip archiving <id>.pdb into ../.temp/symbols/decoders
# Both only skip *extra* steps; the build/test/publish gates are unaffected.
$dst = $env:RUPIX_DIST_DIR
if (-not $dst) { $dst = Join-Path $root "..\..\.temp\decoder-dist" }
New-Item -ItemType Directory -Force -Path $dst | Out-Null

$abiLine = Select-String -Path (Join-Path $root "..\abi\src\lib.rs") -Pattern 'pub const IV_ABI_VERSION: u32 = (\d+);'
if (-not $abiLine) { Write-Error "IV_ABI_VERSION not found in abi/src/lib.rs"; exit 1 }
$abiV = $abiLine.Matches[0].Groups[1].Value

# Optional sibling host build dir (dev setup only; absent in a standalone clone).
# RUPIX_NO_HOST_SYNC=1 skips it: needed where the host tree is not ours to write
# (CI, sandboxes that intercept file deletion), and harmless everywhere else.
$hostDecoders = $null
$hostBuild = Join-Path $root "..\..\.build"
if ($env:RUPIX_NO_HOST_SYNC -eq "1") { $hostBuild = $null }
if ($hostBuild -and (Test-Path $hostBuild)) {
    $hostDecoders = Join-Path $hostBuild "decoders"
    New-Item -ItemType Directory -Force -Path $hostDecoders | Out-Null
}

# Debug symbols for the decoders (forensics only - never shipped next to the DLLs).
# They land in the same .temp root the build already uses, where the host's
# build/dump_sym.py looks for them by default (<repo>/.temp/symbols/decoders).
# RUPIX_NO_SYMBOLS=1 skips the forensics archive: it is a "one-off" directory, and
# archiving needs to DELETE the previous <id>.pdb name first (the versioned files are
# hardlinks to it - overwriting through the link would rewrite an older version's bytes).
# Sandboxes/CI that refuse file deletion set this; nothing about publishing changes.
$symDecoders = $null
$tempRoot = Join-Path $root "..\..\.temp"
if ($env:RUPIX_NO_SYMBOLS -eq "1") { $tempRoot = $null }
if ($tempRoot -and (Test-Path $tempRoot)) {
$symDecoders = Join-Path $tempRoot "symbols\decoders"
    New-Item -ItemType Directory -Force -Path $symDecoders | Out-Null
}

$published = 0
$symArchived = 0
# id -> file name actually placed in the host tree / publish dir this run (see "prune").
$hostKeep = @{}
$dstKeep = @{}

# Publish one decoder: <id> comes from the directory name, the DLL from <targetDir>.
# decoders/ holds only the published tiers; singles (singles/) never get here.
function Publish-One([string]$id, [string]$dir, [string]$targetDir) {
    $ver = $null
    $toml = Join-Path $dir "Cargo.toml"
    if (Test-Path $toml) {
        $m = Select-String -Path $toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
        if ($m) { $ver = $m.Matches[0].Groups[1].Value }
    }
    if (-not $ver) {
        # form-B: version lives in the CMake project() declaration.
        $cml = Join-Path $dir "CMakeLists.txt"
        if (Test-Path $cml) {
            $m = Select-String -Path $cml -Pattern 'project\s*\(.*VERSION\s+([0-9]+\.[0-9]+\.[0-9]+)' | Select-Object -First 1
            if ($m) { $ver = $m.Matches[0].Groups[1].Value }
        }
    }
    if (-not $ver) { Write-Error ("version not found for decoder {0}" -f $id); exit 1 }
    $dll = Join-Path $targetDir "$id.dll"
    if (-not (Test-Path $dll)) {
        # A missing DLL silently loses a format at runtime (fallback chain stops).
        Write-Error ("missing build output: {0}" -f $dll)
        exit 1
    }
    $name = "{0}_{1}.dll" -f $id, $ver
    Copy-Item -Force $dll (Join-Path $dst $name)
    Write-Host ("published {0}" -f $name)
    $script:published++
    $script:dstKeep[$id] = $name
    if ($hostDecoders) {
        # Overwrite in place: same id + same version needs no delete, and a delete
        # can be refused by an external guard (AV holding the DLL, sandboxes).
        Copy-Item -Force (Join-Path $dst $name) (Join-Path $hostDecoders $name)
        $script:hostKeep[$id] = $name
    }
    # The DLL is published as `<id>_<ver>.dll`, but its CodeView record still names
    # the PDB by the COMPILE-TIME crate name (`<id>.pdb`) - that is the only name dbghelp
    # ever looks up. Archive it under that name; dump_sym.py strips the _<ver>
    # segment to get back from a crashing module name to this file.
    if ($symDecoders) {
        $pdb = Join-Path $targetDir "$id.pdb"
        if (Test-Path $pdb) {
            # `<id>.pdb` is the newest one. REMOVE FIRST, then copy: the versioned file
            # below is a hardlink to this same inode, and an in-place overwrite would
            # silently rewrite the older version's archive through that second name.
            # Deleting drops one name; the other name keeps the old bytes alive.
            $cur = Join-Path $symDecoders "$id.pdb"
            if (Test-Path $cur) { Remove-Item $cur -Force }
            Copy-Item -Force $pdb $cur
            # Same-version archive: the published name carries abi + version, which is
            # exactly how dump_sym.py maps "which build crashed" back to one file - so a
            # crash in an OLD decoder version resolves instead of falling back to newest.
            $verPdb = Join-Path $symDecoders ("{0}_{1}.pdb" -f $id, $ver)
            if (Test-Path $verPdb) { Remove-Item $verPdb -Force }
            try {
                New-Item -ItemType HardLink -Path $verPdb -Target $cur | Out-Null
            } catch {
                Copy-Item -Force $cur $verPdb      # cross-volume or no hardlink support
            }
            # Keep the last 3 versions per id: old-enough archives are nobody's problem.
            Get-ChildItem $symDecoders -Filter "$id`_*.pdb" -File |
                Sort-Object LastWriteTime -Descending | Select-Object -Skip 3 |
                ForEach-Object { Remove-Item $_.FullName -Force }
            $script:symArchived++
        } else {
            Write-Host ("  symbols: {0} built without debug info - a crash inside it stays at module+offset" -f $id)
        }
    }
}

Get-ChildItem (Join-Path $root "decoders") -Directory | ForEach-Object {
    Publish-One $_.Name $_.FullName $target
}

# --- host tree: one DLL per id (stale names/versions pruned) ---
# Done as ONE prune pass after every publish, not as a delete-before-copy inside the
# loop: a refused delete (external guard, AV lock, sandbox) must not abort a build whose
# artifacts are already correct. A leftover here is not cosmetic - it silently steals
# the suffix by file name order - so it is reported as a WARNING with the reason.
# The publish dir is what gets uploaded, so it gets the same rule as the host tree:
# exactly one DLL per id. Without this a rename (m132/m139) leaves the old names
# behind and the total size doubles silently.
if ($dstKeep.Count -gt 0) {
    Get-ChildItem $dst -Filter "*.dll" -File | ForEach-Object {
        $staleName = $_.Name
        $stalePath = $_.FullName
        $fid = ($staleName -split "_")[0]
        if ($dstKeep[$fid] -eq $staleName) { return }
        $why = "stale name or version"
        try { Remove-Item $stalePath -Force -ErrorAction Stop } catch { }
        if (Test-Path $stalePath) {
            Write-Host ("  publish: WARNING cannot remove {0} ({1})" -f $staleName, $why)
        } else {
            Write-Host ("  publish: pruned {0} ({1})" -f $staleName, $why)
        }
    }
}

if ($hostDecoders) {
    Get-ChildItem $hostDecoders -Filter "*.dll" -File | ForEach-Object {
        $staleName = $_.Name
        $stalePath = $_.FullName
        $fid = ($staleName -split "_")[0]
        if ($hostKeep[$fid] -eq $staleName) { return }
        $why = "stale version"
        # Judge by the RESULT, not by the exception: a delete shim (recycle bin, AV hook)
        # can report an error after the file is already gone - printing "cannot remove"
        # then would be a lie, and the file list is the ground truth anyway.
        try { Remove-Item $stalePath -Force -ErrorAction Stop } catch { }
        if (Test-Path $stalePath) {
            Write-Host ("  host: WARNING cannot remove {0} ({1}) - it still claims its suffixes" -f $staleName, $why)
        } else {
            Write-Host ("  host: pruned {0} ({1})" -f $staleName, $why)
        }
    }
    $kept = ($hostKeep.GetEnumerator() | Sort-Object Key | ForEach-Object { $_.Value }) -join ", "
    Write-Host ("host decoders: {0} dll(s) - {1}" -f $hostKeep.Count, $kept)
}

# --- size accounting: per-DLL table + total (D1 2026-10-18: no total budget, report only) ---
$totalBytes = 0
Write-Host "--- publish size accounting (one DLL per id, no budget - D1) ---"
foreach ($f in (Get-ChildItem $dst -File | Sort-Object Name)) {
    $totalBytes += $f.Length
    Write-Host ("  {0,-40} {1,8:N2} MB" -f $f.Name, ($f.Length / 1MB))
}
Write-Host ("  {0,-40} {1,8:N2} MB" -f "TOTAL", ($totalBytes / 1MB))

if ($hostDecoders) { Write-Host ("host sync -> {0}" -f $hostDecoders) }
if ($symDecoders) { Write-Host ("symbols   -> {0} ({1} pdb)" -f $symDecoders, $symArchived) }
Write-Host ("done: {0} decoder(s) published to {1} (abi v{2})" -f $published, $dst, $abiV)

# --- catalogue freshness: does decoders.json still describe the DLLs just built? ---
# decoders.json is a FLAT ARRAY, one object per DLL (2026-10-26 slimming: no schema /
# decoders / file; sha256 stays because it rejects same-size mirror swaps - see
# tools/make_index.py).
# Two different things get checked, because they mean different things:
#   * STRUCTURE (a DLL the catalogue does not list, or lists but is not here) is a
#     real gap - the host would 404 or offer a decoder that is gone.
#   * BYTES (size differs) is expected after any relink: the cdylib embeds a
#     fresh CodeView GUID, so a DLL can change without a source edit. Publishing
#     a stale size fails SAFE on the host (refused install), so this one is a
#     one-liner: regenerate before you upload, do not panic.
# Neither is fatal, because the catalogue cannot be regenerated until the release
# tag is known.
$indexPath = Join-Path $root "..\decoders.json"
if (Test-Path $indexPath) {
    $gaps = @()
    $drift = @()
    $catCount = 0
    try {
        # NOT @(...) around the pipeline: in PS 5.1 ConvertFrom-Json emits a
        # top-level JSON array as ONE non-enumerated object, so @(pipeline) wraps
        # that array in a second one - count becomes 1 and $e.id is an array
        # (measured 2026-10-26: "1" and "System.Object[]_System.Object[].dll").
        # Plain assignment keeps Object[]; the -isnot guard covers a one-entry
        # catalogue, where PowerShell may collapse the array to a single object.
        $cat = Get-Content -Raw -Path $indexPath -Encoding UTF8 | ConvertFrom-Json
        if ($cat -isnot [System.Array]) { $cat = @($cat) }
        $catCount = $cat.Count
        $listed = @{}
        foreach ($e in $cat) {
            # filename is derived, not stored: <id>_<version>.dll
            $fname = "{0}_{1}.dll" -f $e.id, $e.version
            $listed[$fname] = $true
            $dll = Join-Path $dst $fname
            if (-not (Test-Path $dll)) {
                $gaps += ("{0}: {1} is listed but not in the publish dir" -f $e.id, $fname)
                continue
            }
            if ((Get-Item $dll).Length -ne [int64]$e.size) {
                $drift += [string]$e.id
                continue
            }
            if ($e.sha256) {
                $got = (Get-FileHash -Path $dll -Algorithm SHA256).Hash.ToLower()
                if ($got -ne ([string]$e.sha256).ToLower()) { $drift += ("{0}(sha)" -f $e.id) }
            }
        }
        foreach ($f in (Get-ChildItem $dst -Filter "*.dll" -File)) {
            if (-not $listed[$f.Name]) { $gaps += ("{0} is built but not in the catalogue" -f $f.Name) }
        }
    } catch {
        $gaps += ("decoders.json unreadable: " + $_.Exception.Message)
    }
    Write-Host ""
    if ($gaps.Count -gt 0) {
        Write-Host "WARNING: decoders.json does not describe this build - fix before publishing:"
        foreach ($g in $gaps) { Write-Host ("  {0}" -f $g) }
        Write-Host ("  fix: py ..\tools\make_index.py")
    } elseif ($drift.Count -gt 0) {
        Write-Host ("catalogue : {0} of {1} size/sha differ (relinked: {2}) - rerun make_index.py before" -f $drift.Count, $catCount, ($drift -join ", "))
        Write-Host "            uploading, or the host refuses the download on a size/sha mismatch"
    } else {
        Write-Host ("catalogue : decoders.json matches the {0} built DLL(s)" -f $catCount)
    }
}
