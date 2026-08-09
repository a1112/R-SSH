[CmdletBinding()]
param(
    [string]$Baseline,
    [string]$Candidate = 'current',
    [string]$Output,
    [switch]$ValidationOnly,
    [switch]$AllowDifferentMachine
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot 'scorecard-common.ps1')

$repoRoot = Get-RsshRepositoryRoot
$schemaPath = Join-Path $PSScriptRoot 'scorecard.schema.json'
if ([string]::IsNullOrWhiteSpace($Baseline)) {
    $Baseline = Join-Path $PSScriptRoot 'baselines\windows-x64-rust-1.89.json'
}
Test-RsshPerformanceSchema -Path $schemaPath
$baselineDocument = Read-RsshPerformanceBaseline -Path $Baseline

if ($ValidationOnly) {
    Write-RsshScorecardResult -Result ([ordered]@{
        ok = $true
        mode = 'validation-only'
        schema_version = $baselineDocument.schema_version
        baseline_commit = $baselineDocument.baseline_commit
    }) -Output $Output
    exit 0
}

if ($Candidate -ne 'current') {
    throw "unsupported candidate '$Candidate'; this runner currently measures the checked-out source as 'current'"
}

$fingerprint = Get-RsshMachineFingerprint
$fingerprintMismatches = @(
    Assert-RsshComparableMachine `
        -Baseline $baselineDocument `
        -Actual $fingerprint `
        -AllowDifferentMachine:$AllowDifferentMachine
)

function Invoke-RsshMeasuredCommand {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Program,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )

    $stopwatch = [Diagnostics.Stopwatch]::StartNew()
    & $Program @Arguments | Out-Host
    $exitCode = $LASTEXITCODE
    $stopwatch.Stop()
    if ($exitCode -ne 0) {
        throw "$Program $($Arguments -join ' ') failed with exit code $exitCode"
    }
    return [long]$stopwatch.ElapsedMilliseconds
}

function Get-RsshDirectoryBytes {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][string]$Path)

    $measurement = Get-ChildItem -LiteralPath $Path -Recurse -File |
        Measure-Object -Property Length -Sum
    if ($null -eq $measurement.Sum) {
        return 0L
    }
    return [long]$measurement.Sum
}

$temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$targetName = "rssh-build-scorecard-$([Guid]::NewGuid().ToString('N'))"
$targetDirectory = [IO.Path]::GetFullPath((Join-Path $temporaryRoot $targetName))
$temporaryPrefix = $temporaryRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
if (-not $targetDirectory.StartsWith($temporaryPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "refusing to create scorecard target outside the temporary directory: $targetDirectory"
}
if ([IO.Path]::GetFileName($targetDirectory) -notlike 'rssh-build-scorecard-*') {
    throw "refusing unexpected scorecard target name: $targetDirectory"
}
$null = New-Item -ItemType Directory -Path $targetDirectory

$previousTarget = $env:CARGO_TARGET_DIR
try {
    Push-Location $repoRoot
    try {
        $null = Invoke-RsshMeasuredCommand -Program 'npm' -Arguments @('--prefix', 'web', 'ci')
        $null = Invoke-RsshMeasuredCommand -Program 'npm' -Arguments @('--prefix', 'web', 'run', 'build')

        $env:CARGO_TARGET_DIR = $targetDirectory
        $cleanCheckMs = Invoke-RsshMeasuredCommand -Program 'cargo' -Arguments @(
            'check', '--locked', '-p', 'rssh-app', '--tests'
        )
        $warmCheckMs = Invoke-RsshMeasuredCommand -Program 'cargo' -Arguments @(
            'check', '--locked', '-p', 'rssh-app', '--tests'
        )
        $null = Invoke-RsshMeasuredCommand -Program 'cargo' -Arguments @(
            'clean', '-p', 'rssh-app'
        )
        $packageRebuildMs = Invoke-RsshMeasuredCommand -Program 'cargo' -Arguments @(
            'check', '--locked', '-p', 'rssh-app', '--tests'
        )
        $testNoRunMs = Invoke-RsshMeasuredCommand -Program 'cargo' -Arguments @(
            'test', '--locked', '--no-run', '-p', 'rssh-app'
        )

        $targetBytes = Get-RsshDirectoryBytes -Path $targetDirectory
        $harnesses = @(
            Get-ChildItem -LiteralPath (Join-Path $targetDirectory 'debug\deps') `
                -Filter 'rssh_app-*.exe' -File -ErrorAction SilentlyContinue
        )
        $largestHarnessBytes = if ($harnesses.Count -eq 0) {
            0L
        } else {
            [long](($harnesses | Sort-Object Length -Descending | Select-Object -First 1).Length)
        }
        $unitExecutionMs = Invoke-RsshMeasuredCommand -Program 'cargo' -Arguments @(
            'test', '--locked', '-p', 'rssh-app', '--bin', 'rssh-app', '--quiet'
        )
        $null = Invoke-RsshMeasuredCommand -Program 'cargo' -Arguments @(
            'build', '--locked', '--release', '-p', 'rssh-app'
        )
        $releaseExecutable = Join-Path $targetDirectory 'release\rssh-app.exe'
        if (-not (Test-Path -LiteralPath $releaseExecutable -PathType Leaf)) {
            throw "release executable was not produced: $releaseExecutable"
        }
        $releaseExecutableBytes = [long](Get-Item -LiteralPath $releaseExecutable).Length
    } finally {
        Pop-Location
    }

    $candidateMetrics = [ordered]@{
        clean_check_ms = $cleanCheckMs
        warm_check_ms = $warmCheckMs
        package_rebuild_ms = $packageRebuildMs
        test_no_run_ms = $testNoRunMs
        target_bytes = $targetBytes
        largest_harness_bytes = $largestHarnessBytes
        unit_execution_ms = $unitExecutionMs
        release_executable_bytes = $releaseExecutableBytes
    }
    $violations = [System.Collections.Generic.List[object]]::new()
    foreach ($gate in @(
        @('clean_check_ms', 'clean_check_ms_max'),
        @('package_rebuild_ms', 'package_rebuild_ms_max'),
        @('test_no_run_ms', 'test_no_run_ms_max'),
        @('target_bytes', 'target_bytes_max'),
        @('largest_harness_bytes', 'largest_harness_bytes_max'),
        @('unit_execution_ms', 'unit_execution_ms_max'),
        @('release_executable_bytes', 'release_executable_bytes_max')
    )) {
        $observed = [long]$candidateMetrics[$gate[0]]
        $limit = [long]$baselineDocument.build.gates.($gate[1])
        if ($observed -gt $limit) {
            $violations.Add([ordered]@{
                metric = $gate[0]
                observed = $observed
                expected = "<=$limit"
            })
        }
    }

    $result = [ordered]@{
        ok = $violations.Count -eq 0
        mode = 'build'
        schema_version = $baselineDocument.schema_version
        baseline_commit = $baselineDocument.baseline_commit
        candidate = $Candidate
        machine = $fingerprint
        machine_mismatches = $fingerprintMismatches
        baseline = $baselineDocument.build.baseline
        gates = $baselineDocument.build.gates
        measured = $candidateMetrics
        threshold_violations = @($violations)
    }
    Write-RsshScorecardResult -Result $result -Output $Output
    if (-not $result.ok) {
        exit 1
    }
} finally {
    if ($null -eq $previousTarget) {
        Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    } else {
        $env:CARGO_TARGET_DIR = $previousTarget
    }
    $resolvedTarget = [IO.Path]::GetFullPath($targetDirectory)
    if (-not $resolvedTarget.StartsWith($temporaryPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "refusing to clean scorecard target outside the temporary directory: $resolvedTarget"
    }
    if ([IO.Path]::GetFileName($resolvedTarget) -notlike 'rssh-build-scorecard-*') {
        throw "refusing to clean unexpected scorecard target: $resolvedTarget"
    }
    if (Test-Path -LiteralPath $resolvedTarget) {
        Remove-Item -LiteralPath $resolvedTarget -Recurse -Force
    }
}
