param(
    [Parameter(Mandatory = $true)][string]$Archive,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][ValidateSet("windows", "linux", "macos")][string]$Platform,
    [Parameter(Mandatory = $true)][string]$Architecture
)

$ErrorActionPreference = "Stop"
$archivePath = (Resolve-Path $Archive).Path
$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("colosseum-gui-smoke-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $scratch | Out-Null
try {
    if ($archivePath.EndsWith(".zip", [StringComparison]::OrdinalIgnoreCase)) {
        Expand-Archive -LiteralPath $archivePath -DestinationPath $scratch
    } else {
        & tar -xzf $archivePath -C $scratch
        if ($LASTEXITCODE -ne 0) { throw "tar failed with exit code $LASTEXITCODE" }
    }
    $name = "colosseum-gui-$Version-$Platform-$Architecture"
    $root = Join-Path $scratch $name
    $binaryName = if ($Platform -eq "windows") { "colosseum.exe" } else { "colosseum" }
    $binary = Join-Path $root $binaryName
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "GUI archive is missing $binaryName"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $root "LICENSE") -PathType Leaf)) {
        throw "GUI archive is missing LICENSE"
    }
    $stdoutPath = Join-Path $scratch "version.stdout"
    $stderrPath = Join-Path $scratch "version.stderr"
    $process = Start-Process -FilePath $binary -ArgumentList "--version" -Wait -PassThru `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
    # -Wait returns when the process exits, but Windows can hold the image file
    # open a moment longer, which would then fail the cleanup below.
    $process.WaitForExit()
    $reported = (Get-Content $stdoutPath -Raw) ?? ""
    $diagnostics = (Get-Content $stderrPath -Raw) ?? ""
    if ($process.ExitCode -ne 0) { throw "GUI --version failed: $diagnostics" }
    if ($diagnostics.Trim()) { throw "GUI --version wrote unexpected stderr: $diagnostics" }
    if ($reported.Trim() -ne "colosseum $Version") {
        throw "GUI version mismatch: expected 'colosseum $Version', got '$($reported.Trim())'"
    }
    Write-Host "Exact GUI archive smoke passed: $name"
} finally {
    # Cleanup is not the thing being verified: a temporary directory Windows
    # still holds open must not turn a passing archive into a failing one.
    for ($attempt = 1; $attempt -le 10; $attempt++) {
        try {
            Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction Stop
            break
        } catch {
            if ($attempt -eq 10) {
                Write-Warning "could not remove the smoke directory $scratch : $_"
            } else {
                Start-Sleep -Milliseconds 200
            }
        }
    }
}
