[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$Fastchess,

    [Parameter(Mandatory)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$Cutechess,

    [Parameter(Mandatory)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$EngineA,

    [Parameter(Mandatory)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$EngineB,

    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$EngineAName,

    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$EngineBName,

    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$EngineASource,

    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$EngineBSource,

    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$EngineALicense,

    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$EngineBLicense,

    [ValidatePattern('^\d+(?:\.\d+)?\+\d+(?:\.\d+)?$')]
    [string]$TimeControl = '0.2+0.01',

    [ValidateRange(1, 1000000)]
    [int]$Rounds = 2,

    [ValidateSet('normalized', 'logistic')]
    [string]$FastchessModel = 'normalized',

    [ValidateNotNullOrEmpty()]
    [string]$OutputDirectory = 'tests/fixtures/statistics/external/generated'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Resolve-Executable([string]$Path) {
    return (Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
}

function Get-Identity([string]$Path, [string]$Source, [string]$License) {
    $resolvedPath = Resolve-Executable $Path
    return [ordered]@{
        file_name = Split-Path -Leaf $resolvedPath
        sha256 = (Get-FileHash -LiteralPath $resolvedPath -Algorithm SHA256).Hash.ToLowerInvariant()
        source = $Source
        license = $License
    }
}

function Invoke-LoggedRunner(
    [string]$Executable,
    [string[]]$Arguments,
    [string]$LogPath,
    [string]$WorkingDirectory
) {
    Push-Location -LiteralPath $WorkingDirectory
    try {
        & $Executable @Arguments 2>&1 | Tee-Object -FilePath $LogPath
        if ($LASTEXITCODE -ne 0) {
            throw "Runner failed ($LASTEXITCODE): $Executable"
        }
    }
    finally {
        Pop-Location
    }
}

$fastchessPath = Resolve-Executable $Fastchess
$cutechessPath = Resolve-Executable $Cutechess
$engineAPath = Resolve-Executable $EngineA
$engineBPath = Resolve-Executable $EngineB
$outputPath = [IO.Path]::GetFullPath($OutputDirectory)

if (Test-Path -LiteralPath $outputPath) {
    throw "Output directory already exists: $outputPath. Choose a new output directory; existing evidence is never overwritten."
}
New-Item -ItemType Directory -Path $outputPath | Out-Null

$fastchessVersion = (& $fastchessPath -version 2>&1 | Out-String).Trim()
$cutechessVersion = (& $cutechessPath -version 2>&1 | Out-String).Trim()

# Keep the schedule intentionally small: it proves the external-output shape,
# not engine strength. A later parity fixture must use a representative book,
# time control and non-degenerate official sample.
$fastchessPgn = Join-Path $outputPath 'fastchess.pgn'
$runnerWorkDirectory = Join-Path ([IO.Path]::GetTempPath()) "colosseum-statistics-fixture-$([Guid]::NewGuid())"
New-Item -ItemType Directory -Path $runnerWorkDirectory | Out-Null

try {
$fastchessArguments = @(
    '-engine', "cmd=$engineAPath", "name=$EngineAName",
    '-engine', "cmd=$engineBPath", "name=$EngineBName",
    '-each', "tc=$TimeControl",
    '-rounds', $Rounds, '-repeat', '-concurrency', '1',
    '-pgnout', "file=$fastchessPgn",
    '-sprt', 'elo0=-5', 'elo1=5', 'alpha=0.05', 'beta=0.05', "model=$FastchessModel",
    '-output', 'format=cutechess'
)
    Invoke-LoggedRunner $fastchessPath $fastchessArguments (Join-Path $outputPath 'fastchess.console.txt') $runnerWorkDirectory

$cutechessPgn = Join-Path $outputPath 'cutechess.pgn'
$cutechessArguments = @(
    '-engine', "name=$EngineAName", "cmd=$engineAPath", 'proto=uci',
    '-engine', "name=$EngineBName", "cmd=$engineBPath", 'proto=uci',
    '-each', "tc=$TimeControl",
    '-rounds', $Rounds, '-games', '2', '-repeat',
    '-pgnout', $cutechessPgn, 'fi',
    '-sprt', 'elo0=-5', 'elo1=5', 'alpha=0.05', 'beta=0.05'
)
    Invoke-LoggedRunner $cutechessPath $cutechessArguments (Join-Path $outputPath 'cutechess.console.txt') $runnerWorkDirectory
}
finally {
    Remove-Item -LiteralPath $runnerWorkDirectory -Recurse -Force -ErrorAction SilentlyContinue
}

$provenance = [ordered]@{
    schema_version = 1
    generated_utc = [DateTime]::UtcNow.ToString('O')
    host = [ordered]@{
        os = [Environment]::OSVersion.VersionString
        architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    }
    tools = [ordered]@{
        fastchess = Get-Identity $fastchessPath 'https://github.com/Disservin/fastchess' 'MIT'
        cutechess = Get-Identity $cutechessPath 'https://github.com/cutechess/cutechess' 'GPL-3.0-or-later'
    }
    engines = [ordered]@{
        a = Get-Identity $engineAPath $EngineASource $EngineALicense
        b = Get-Identity $engineBPath $EngineBSource $EngineBLicense
    }
    conditions = [ordered]@{
        rounds = $Rounds
        pairing = 'two games per round, colours reversed by runner'
        time_control = $TimeControl
        book = $null
        concurrency = 1
        fastchess_sprt = [ordered]@{ model = $FastchessModel; elo0 = -5; elo1 = 5; alpha = 0.05; beta = 0.05 }
        cutechess_sprt = [ordered]@{ elo0 = -5; elo1 = 5; alpha = 0.05; beta = 0.05 }
    }
    artifacts = [ordered]@{
        fastchess_console_sha256 = (Get-FileHash -LiteralPath (Join-Path $outputPath 'fastchess.console.txt') -Algorithm SHA256).Hash.ToLowerInvariant()
        fastchess_pgn_sha256 = (Get-FileHash -LiteralPath $fastchessPgn -Algorithm SHA256).Hash.ToLowerInvariant()
        cutechess_console_sha256 = (Get-FileHash -LiteralPath (Join-Path $outputPath 'cutechess.console.txt') -Algorithm SHA256).Hash.ToLowerInvariant()
        cutechess_pgn_sha256 = (Get-FileHash -LiteralPath $cutechessPgn -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}
$provenance | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $outputPath 'provenance.json') -Encoding utf8NoBOM
