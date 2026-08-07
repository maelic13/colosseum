param(
    [Parameter(Mandatory = $true)][string]$Archive,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][ValidateSet("windows", "linux", "macos")][string]$Platform,
    [Parameter(Mandatory = $true)][string]$Architecture
)

$ErrorActionPreference = "Stop"
$archivePath = (Resolve-Path $Archive).Path
$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("colosseum-cli-smoke-" + [guid]::NewGuid())
$unpack = Join-Path $scratch "unpack"
$work = Join-Path $scratch "work"
New-Item -ItemType Directory -Path $unpack, $work | Out-Null

try {
    if ($archivePath.EndsWith(".zip", [StringComparison]::OrdinalIgnoreCase)) {
        Expand-Archive -LiteralPath $archivePath -DestinationPath $unpack
    } elseif ($archivePath.EndsWith(".tar.gz", [StringComparison]::OrdinalIgnoreCase)) {
        & tar -xzf $archivePath -C $unpack
        if ($LASTEXITCODE -ne 0) { throw "tar failed with exit code $LASTEXITCODE" }
    } else {
        throw "unsupported CLI archive: $archivePath"
    }

    $expectedName = "colosseum-cli-$Version-$Platform-$Architecture"
    $root = Join-Path $unpack $expectedName
    if (-not (Test-Path -LiteralPath $root -PathType Container)) {
        throw "archive does not contain expected root directory $expectedName"
    }
    $topEntries = @(Get-ChildItem -LiteralPath $unpack)
    if ($topEntries.Count -ne 1 -or $topEntries[0].Name -ne $expectedName) {
        throw "archive must contain exactly one top-level directory"
    }

    $binaryName = if ($Platform -eq "windows") { "colosseum-cli.exe" } else { "colosseum-cli" }
    $binary = Join-Path $root $binaryName
    foreach ($required in @($binary, (Join-Path $root "LICENSE"), (Join-Path $root "README.md"), (Join-Path $root "docs/cli/quickstart.md"), (Join-Path $root "docs/cli/command-reference.md"))) {
        if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
            throw "required archive file is missing: $required"
        }
    }

    function Invoke-Cli {
        param([string[]]$Arguments, [string]$Label)
        $stderrPath = Join-Path $work "$Label.stderr"
        Push-Location $work
        try {
            $stdout = @(& $binary @Arguments 2> $stderrPath)
            $exitCode = $LASTEXITCODE
        } finally {
            Pop-Location
        }
        if ($exitCode -ne 0) {
            $diagnostics = if (Test-Path $stderrPath) { (Get-Content $stderrPath -Raw) ?? "" } else { "" }
            throw "$Label failed with exit code ${exitCode}: $diagnostics"
        }
        $diagnostics = if (Test-Path $stderrPath) { ((Get-Content $stderrPath -Raw) ?? "").Trim() } else { "" }
        if ($diagnostics) { throw "$Label wrote unexpected stderr: $diagnostics" }
        return ($stdout -join [Environment]::NewLine)
    }

    $reported = Invoke-Cli -Arguments @("--version") -Label "version"
    if ($reported.Trim() -ne "colosseum-cli $Version") {
        throw "version mismatch: expected 'colosseum-cli $Version', got '$($reported.Trim())'"
    }
    $help = Invoke-Cli -Arguments @("--help") -Label "help"
    if ($help -notmatch "A headless harness for inspecting, testing and comparing ordinary UCI chess-engine executables") {
        throw "help smoke did not contain the expected product description"
    }

    $selfTest = Invoke-Cli -Arguments @("--json", "self-test") -Label "self-test"
    $selfTestJson = $selfTest | ConvertFrom-Json
    if ($selfTestJson.type -ne "self-test") { throw "self-test stdout has the wrong JSON type" }

    $plan = Invoke-Cli -Arguments @(
        "--json", "stats", "plan", "fixed",
        "--objective", "difference",
        "--model", "normalized",
        "--effect-or-margin", "3",
        "--distribution", "0.05,0.15,0.6,0.15,0.05"
    ) -Label "json-workflow"
    $planJson = $plan | ConvertFrom-Json
    if ($planJson.type -ne "stats-fixed-plan") { throw "deterministic workflow stdout has the wrong JSON type" }

    Write-Host "Exact archive smoke passed: $expectedName"
} finally {
    if (Test-Path -LiteralPath $scratch) {
        Remove-Item -LiteralPath $scratch -Recurse -Force
    }
}
