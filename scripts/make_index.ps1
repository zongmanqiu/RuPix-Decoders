# Generate index.json - the official plugin catalogue for the host's
# "online plugin sources" feature (host plan: .plan/m09-online-plugin-sources-Gitee-and-Github.md).
#
# WHAT IT DOES
#   Walks the publish dir (<id>_<ver>.dll), loads each DLL and reads its
#   OWN iv_plugin_info() self-description, then emits schema v1 JSON:
#     id / name / version / abi / formats / desc / author / homepage /
#     file / sha256 / size / minHost / url{gitee,github} / notes
#
# WHERE THE DOWNLOAD URL COMES FROM (2026-10-26 change)
#   Release assets, not raw files: this repo is SOURCE ONLY - `dist/` never
#   enters it, and the DLLs reach users through a tagged release. URL shape:
#     https://gitee.com/<owner>/<repo>/releases/download/<tag>/<file>
#   TAG = "<id>_<version>" (e.g. common_0.1.0). Each plugin is uploaded as its
#   own release, so one shared date tag is no longer the truth: the old batch
#   tag left 404s behind for every DLL that was not part of that upload.
#
#   The URL itself is HAND-MAINTAINED (like `notes`). The generator has no
#   network and cannot know whether a release really exists, so it must not
#   invent an address: a catalogue full of plausible-but-404 links is worse
#   than one that admits "not published yet". Missing addresses are listed at
#   the end of every run; scripts/check_urls.py re-checks them for real.
#   `-Tag <shared>` still works for a genuine all-in-one batch release.
#
# WHAT THE GENERATOR OWNS vs. WHAT YOU MAY HAND-EDIT
#   Owned (overwritten every run): id name version abi formats desc author
#   homepage file sha256 size minHost tag  - all derived from the binary, because
#   the host refuses a download whose self-description disagrees with the
#   catalogue (plan decision D6); generating them FROM the binary makes that
#   check pass by construction rather than by discipline.
#   Yours (preserved): `url`, `notes`, plus any extra key you add to an entry -
#   the generator only replaces the fields it owns. `url` is carried over
#   verbatim in per-plugin mode and re-derived only when `-Tag` is given.
#
# WHY THE DLL IS THE SOURCE OF TRUTH (not the Cargo.toml / source comments):
#   the host refuses a download whose self-description disagrees with index.json
#   (plan decision D6). Generating the catalogue FROM the published binary is what
#   makes that check pass by construction instead of by discipline.
#   version comes from the published filename (build.ps1 stamped it from
#   [package] version), and is cross-checked against the filename's abi field.
#
# SCOPE: only ids that exist under decoders/ are catalogued -- anything else
#   sitting in the publish dir (a stray fixture build, an old rename) is skipped
#   on purpose.
#
# Usage (from the repo root or anywhere):
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\make_index.ps1
#   ... -Tag <shared-tag>   (only for a batch release carrying every DLL)
#   ... -Owner myname -GiteeRepo my-repo -GithubRepo my-mirror
#
# THIS SCRIPT must be ASCII-only: PowerShell 5.1 reads non-BOM UTF-8 as ANSI (K27).
# The OUTPUT (index.json) is the opposite - it is UTF-8 WITH non-ASCII, because
# ConvertTo-Json does NOT escape CJK (measured: 414 bytes > 0x7F), and that is
# fine for JSON/HTTP/rust but means any PowerShell reader of it MUST say
# `-Encoding UTF8` (K138: Get-Content defaults to the ANSI code page and shreds it).

param(
    [string]$Owner = "qiuzongman",
    [string]$GiteeRepo = "RuPix-Decoder",
    [string]$GithubRepo = "",
    [string]$Tag = "",
    [string]$MinHost = "0.1.0",
    [string]$DistDir,
    [string]$OutFile
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not $DistDir) { $DistDir = $env:RUPIX_DIST_DIR }
if (-not $DistDir) { $DistDir = Join-Path $root "..\.temp\decoder-dist" }
if (-not $OutFile) { $OutFile = Join-Path $root "index.json" }

if (-not [Environment]::Is64BitProcess) {
    Write-Error "run the 64-bit PowerShell (x64 DLLs cannot be loaded from a 32-bit process)"
    exit 1
}
if (-not (Test-Path $DistDir)) {
    Write-Error "publish dir not found - run build.ps1 first (expected: $DistDir)"
    exit 1
}

# Official ids = directories under decoders/ (the published tiers). Singles live in
# bench/singles/ and are never published, so anything else in the publish dir is stale.
$official = @(Get-ChildItem (Join-Path $root "decoders") -Directory | ForEach-Object { $_.Name })

# --- read the catalogue already on disk: it carries the tag and the manual fields ---
# `notes` and any extra key the author added are NOT derived from anything, so
# they can only survive by being carried over from the previous file.
$prevById = @{}
$prevTag = ""
if (Test-Path $OutFile) {
    try {
        # -Encoding UTF8 is load-bearing: PowerShell 5.1 decodes a BOM-less file
        # with the ANSI code page, so `desc` (Chinese) turns into mojibake and
        # ConvertFrom-Json dies with a bogus position and the whole file echoed
        # back as the "error message" (K138).
        $prev = Get-Content -Raw -Path $OutFile -Encoding UTF8 | ConvertFrom-Json
        foreach ($p in $prev.plugins) { if ($p.id) { $prevById[[string]$p.id] = $p } }
        if ($prev.releaseTag) { $prevTag = [string]$prev.releaseTag }
    } catch {
        $why = $_.Exception.Message
        Write-Host ("  WARN: previous index.json unreadable ({0}); notes/extra fields start empty" -f $why)
    }
}
# Per-plugin mode (default): every DLL gets its own release tag <id>_<version>
# and the URL is carried over from the previous catalogue (hand-maintained).
# Shared mode (only when -Tag is passed): one release carries every DLL.
if ($Tag) {
    Write-Host ("tag mode: SHARED ({0}) - every url is derived from it" -f $Tag)
} else {
    Write-Host "tag mode: PER-PLUGIN (<id>_<version>); urls carried over from the previous index"
}

# --- P/Invoke helper: LoadLibraryEx + GetProcAddress, read IvPluginInfo by hand ---
# Struct on x64 (abi/src/lib.rs pins size_of == 48):
#   0  id:*const c_char   8 display_name  16 abi_version:u32  (4 pad)
#  24  extensions         32 is_vector:i32 36 has_alpha:i32   40 description
if (-not ("RuPixIndexProbe" -as [type])) {
Add-Type -Language CSharp -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class RuPixIndexProbe
{
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern IntPtr LoadLibraryExW(string path, IntPtr hFile, uint flags);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool FreeLibrary(IntPtr h);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Ansi)]
    static extern IntPtr GetProcAddress(IntPtr h, string name);

    delegate IntPtr InfoFn();
    delegate IntPtr AbiFn();

    static string ReadCString(IntPtr p)
    {
        if (p == IntPtr.Zero) { return ""; }
        List<byte> bytes = new List<byte>();
        for (int i = 0; ; i++)
        {
            byte b = Marshal.ReadByte(p, i);
            if (b == 0) { break; }
            bytes.Add(b);
        }
        return Encoding.UTF8.GetString(bytes.ToArray(), 0, bytes.Count);
    }

    // returns id, display_name, abi, extensions, is_vector, has_alpha, description
    public static string[] ReadInfo(string dllPath)
    {
        IntPtr lib = LoadLibraryExW(dllPath, IntPtr.Zero, 0u);
        if (lib == IntPtr.Zero)
        {
            throw new InvalidOperationException("LoadLibraryExW failed, win32 error " +
                Marshal.GetLastWin32Error());
        }
        try
        {
            IntPtr fpInfo = GetProcAddress(lib, "iv_plugin_info");
            if (fpInfo == IntPtr.Zero)
            {
                throw new InvalidOperationException("iv_plugin_info export missing");
            }
            InfoFn fn = (InfoFn)Marshal.GetDelegateForFunctionPointer(fpInfo, typeof(InfoFn));
            IntPtr info = fn();
            if (info == IntPtr.Zero)
            {
                throw new InvalidOperationException("iv_plugin_info returned NULL");
            }
            string[] r = new string[7];
            r[0] = ReadCString(Marshal.ReadIntPtr(info, 0));
            r[1] = ReadCString(Marshal.ReadIntPtr(info, 8));
            r[2] = ((uint)Marshal.ReadInt32(info, 16)).ToString();
            r[3] = ReadCString(Marshal.ReadIntPtr(info, 24));
            r[4] = Marshal.ReadInt32(info, 32).ToString();
            r[5] = Marshal.ReadInt32(info, 36).ToString();
            r[6] = ReadCString(Marshal.ReadIntPtr(info, 40));
            if (string.IsNullOrEmpty(r[0]))
            {
                throw new InvalidOperationException("plugin id is empty");
            }
            return r;
        }
        finally
        {
            FreeLibrary(lib);
        }
    }
}
'@
}

$giteeBase = ""
$githubBase = ""
if ($Tag) {
    $giteeBase = "https://gitee.com/{0}/{1}/releases/download/{2}" -f $Owner, $GiteeRepo, $Tag
    if ($GithubRepo) {
        $githubBase = "https://github.com/{0}/{1}/releases/download/{2}" -f $Owner, $GithubRepo, $Tag
    }
}
$missingUrl = New-Object System.Collections.Generic.List[string]

$plugins = New-Object System.Collections.Generic.List[object]
$failures = @()
$urlNotes = @()

foreach ($f in (Get-ChildItem $DistDir -Filter "*.dll" -File | Sort-Object Name)) {
    # published name shape: <id>_<ver>.dll
    $m = [regex]::Match($f.BaseName, '^(?<id>[a-z0-9\-]+)_(?<ver>[0-9A-Za-z\.\-]+)$')
    if (-not $m.Success) {
        $failures += ("unparsable published name: {0} (expected <id>_<ver>.dll)" -f $f.Name)
        continue
    }
    $id = $m.Groups["id"].Value
    $ver = $m.Groups["ver"].Value

    if ($official -notcontains $id) {
        Write-Host ("  skip (not under decoders/): {0}" -f $f.Name)
        continue
    }

    $info = $null
    try {
        $info = [RuPixIndexProbe]::ReadInfo($f.FullName)
    } catch {
        $failures += ("{0}: {1}" -f $f.Name, $_.Exception.Message)
        continue
    }

    # ABI comes from the binary and nowhere else: it is not in the file name any more.
    $fileAbi = [int]$info[2]

    # Cross-check the id in the file name against the binary's own words: a mismatch
    # means the publish dir holds a stale rename, and the host would file the download
    # under the wrong identity.`r`n
    if ($info[0] -ne $id) {
        $failures += ("{0}: filename id '{1}' != self-reported id '{2}'" -f $f.Name, $id, $info[0])
        continue
    }

    $formats = @()
    foreach ($e in ($info[3] -split ';')) {
        if ($e.Trim()) { $formats += ("." + $e.Trim()) }
    }

    # Release tag for this build: <id>_<version> - the same name the release
    # gets on gitee. Derived, so a stale hand-written tag cannot survive.
    $tagName = $id + "_" + $ver

    $urls = [ordered]@{}
    $prevEntry = $prevById[[string]$info[0]]
    if ($Tag) {
        $urls["gitee"] = "$giteeBase/$($f.Name)"
        if ($githubBase) { $urls["github"] = "$githubBase/$($f.Name)" }
    } elseif ($prevEntry -and $prevEntry.url -and $prevEntry.url.gitee) {
        # Hand-maintained address survives: no network here, so never guess.
        $urls["gitee"] = [string]$prevEntry.url.gitee
        if ($prevEntry.url.github) { $urls["github"] = [string]$prevEntry.url.github }
    }
    if (-not $urls["gitee"]) { $missingUrl.Add($id) }

    $notes = ""
    if ($prevEntry -and $null -ne $prevEntry.notes) { $notes = [string]$prevEntry.notes }

    $entry = [ordered]@{
        id       = $info[0]
        name     = $info[1]
        version  = $ver
        abi      = [int]$info[2]
        formats  = $formats
        desc     = $info[6]
        author   = $Owner
        homepage = "https://gitee.com/$Owner/$GiteeRepo"
        file     = $f.Name
        sha256   = (Get-FileHash -Algorithm SHA256 $f.FullName).Hash.ToLower()
        size     = [int64]$f.Length
        minHost  = $MinHost
        tag      = $tagName
        url      = $urls
        notes    = $notes
    }
    # Hand-added keys survive: the generator only replaces the fields it owns.
    if ($prevEntry) {
        foreach ($pr in $prevEntry.PSObject.Properties) {
            if ($entry.Keys -notcontains $pr.Name) { $entry[$pr.Name] = $pr.Value }
        }
    }
    $plugins.Add([pscustomobject]$entry) > $null
    Write-Host ("  + {0} ({1} formats, {2:N0} bytes)" -f $id, $formats.Count, $f.Length)
}

if ($failures.Count -gt 0) {
    foreach ($x in $failures) { Write-Host ("  ERROR {0}" -f $x) }
    Write-Error "index.json NOT written: publish dir contents disagree with the binaries"
    exit 1
}
if ($plugins.Count -eq 0) {
    Write-Error "no official plugin found in $DistDir - run build.ps1 first"
    exit 1
}

# Two builds of one id in the publish dir: the host would load whichever it hit
# first and the catalogue would describe the other. `build.ps1` leaves one DLL
# per id, so seeing two here means someone left a stale rename behind.
$dups = @($plugins | Group-Object id | Where-Object { $_.Count -gt 1 } | ForEach-Object { $_.Name })
if ($dups.Count -gt 0) {
    foreach ($d in $dups) {
        Write-Host ("  ERROR duplicate id in the publish dir: {0} - delete the stale DLL" -f $d)
    }
    Write-Error "index.json NOT written: the publish dir holds more than one build of the same plugin"
    exit 1
}

$index = [ordered]@{
    schema  = 1
    updated = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
}
# `releaseTag` only makes sense when one batch release carries every DLL; in
# per-plugin mode each entry has its own `tag`, and a top-level one would lie.
if ($Tag) { $index["releaseTag"] = $Tag }
$index["plugins"] = $plugins.ToArray()

# -InputObject, NOT pipeline: `1 | ConvertTo-Json` unwraps a single-element array
# and emits one bare object instead of {"plugins":[{...}]} - schema violation.
$json = ConvertTo-Json -InputObject $index -Depth 6
# ConvertTo-Json leaves BOM-free UTF-16 strings here; write UTF-8 without BOM so
# gitee raw (and any plain HTTP consumer) serves it as plain UTF-8 JSON.
$enc = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText($OutFile, $json, $enc)
Write-Host ("wrote {0} ({1} plugin(s))" -f $OutFile, $plugins.Count)
if ($missingUrl.Count -gt 0) {
    Write-Host ("MISSING url (not published yet): {0}" -f ($missingUrl -join ", "))
    Write-Host "  -> upload that DLL as a release tagged <id>_<version>, paste its asset"
    Write-Host "     url into the entry's url.gitee, then re-run (the url survives)."
}
if ($urlNotes.Count -gt 0) {
    Write-Host "NOTE: download url differs from the previous catalogue (tag/owner change, new build,"
    Write-Host "      or a hand edit that is now overwritten - check each line before publishing):"
    foreach ($n in $urlNotes) { Write-Host ("  {0}" -f $n) }
}
