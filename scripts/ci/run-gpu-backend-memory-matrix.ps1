[CmdletBinding()]
param(
    [ValidateSet("debug", "release")]
    [string] $Profile = "release",
    [ValidateRange(0, 100)]
    [int] $Warmups = 5,
    [ValidateRange(1, 1000)]
    [int] $Samples = 30,
    [string] $OutputDirectory = "artifacts/gpu-backend-memory-matrix",
    [switch] $SkipBuild
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false
$report_only_target_bytes = 47185920 # 45 MiB.
$probes = @("cpu", "dx12", "vulkan", "gl")
$profileDirectory = if ($Profile -eq "release") { "release" } else { "debug" }
$executableSuffix = if ($IsWindows) { ".exe" } else { "" }
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "../..")).Path
$targetRoot = if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
    Join-Path $repoRoot "target"
} elseif ([System.IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
    [System.IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
} else {
    [System.IO.Path]::GetFullPath((Join-Path $repoRoot $env:CARGO_TARGET_DIR))
}
$app = Join-Path (Join-Path $targetRoot $profileDirectory) "rssh-app$executableSuffix"
$launcher = Join-Path (Join-Path $targetRoot $profileDirectory) "rssh-bench-launcher$executableSuffix"
$rawDirectory = Join-Path $OutputDirectory "raw"
$aggregatePath = Join-Path $OutputDirectory "aggregate.json"

if (-not $SkipBuild) {
    $profileArguments = @()
    if ($Profile -eq "release") {
        $profileArguments += "--release"
    }
    Push-Location $repoRoot
    try {
        cargo build --locked -p rssh-app @profileArguments
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed for rssh-app with exit code $LASTEXITCODE"
        }
        cargo build --locked -p rssh-diagnostics --bin rssh-bench-launcher @profileArguments
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed for rssh-bench-launcher with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}
if (-not (Test-Path -LiteralPath $app -PathType Leaf)) {
    throw "rssh-app executable is missing from the selected Cargo target/profile"
}
if (-not (Test-Path -LiteralPath $launcher -PathType Leaf)) {
    throw "rssh-bench-launcher executable is missing from the selected Cargo target/profile"
}

New-Item -ItemType Directory -Force -Path $rawDirectory | Out-Null
$previousScale = $env:RSSH_BENCHMARK_WINDOW_SCALE_FACTOR
$env:RSSH_BENCHMARK_WINDOW_SCALE_FACTOR = "1"

function Get-NearestRankPercentile([UInt64[]] $Values, [double] $Percentile) {
    if ($Values.Count -eq 0) {
        throw "nearest-rank percentile requires at least one value"
    }
    $ordered = @($Values | Sort-Object)
    $rank = [Math]::Ceiling($Percentile * $ordered.Count)
    return [UInt64] $ordered[[Math]::Max(0, $rank - 1)]
}

function Get-SafeFailureMessage([System.Management.Automation.ErrorRecord] $Failure) {
    $message = $Failure.Exception.Message
    foreach ($path in @($repoRoot, $targetRoot, $OutputDirectory)) {
        if (-not [string]::IsNullOrWhiteSpace($path)) {
            $message = $message.Replace($path, "[path]")
        }
    }
    return $message
}

function Assert-ProbeRecord([string] $Probe, [object] $Record, [int] $ExitCode) {
    if ($Record.schema -ne "rssh.diagnostics/v2") {
        throw "unexpected diagnostics schema '$($Record.schema)'"
    }
    if ($ExitCode -ne 0 -or $Record.failures.Count -ne 0 -or $Record.readiness.status -ne "ready") {
        $details = @($Record.failures | ForEach-Object { "$($_.code) [$($_.phase)]: $($_.message)" })
        if ($details.Count -eq 0) {
            $details = @("exit code $ExitCode; readiness=$($Record.readiness.status)")
        }
        throw "probe run failed: $($details -join '; ')"
    }
    if (
        $Record.configuration.stabilization_ms -ne 5000 -or
        $Record.configuration.sample_interval_ms -ne 100 -or
        $Record.configuration.sample_count -ne 10 -or
        $Record.configuration.columns -ne 80 -or
        $Record.configuration.rows -ne 24 -or
        $Record.configuration.scale_factor_milli -ne 1000
    ) {
        throw "launcher configuration mismatch"
    }
    if (
        $Record.memory.metric -ne "windows_private_working_set_bytes" -or
        $Record.memory.unit -ne "bytes" -or
        $Record.memory.samples.Count -ne 10
    ) {
        throw "memory schema/sample count mismatch"
    }

    if ($Probe -eq "cpu") {
        if ($Record.configuration.requested_renderer -ne "cpu") {
            throw "requested renderer mismatch for CPU probe"
        }
        if ($Record.configuration.PSObject.Properties.Name -contains "requested_gpu_backend") {
            throw "CPU probe unexpectedly requested a GPU backend"
        }
        if ($Record.renderer.final -ne "cpu") {
            throw "final renderer mismatch: CPU probe observed '$($Record.renderer.final)'"
        }
        if ($Record.renderer.PSObject.Properties.Name -contains "backend") {
            throw "CPU probe unexpectedly reported an actual GPU backend"
        }
        return
    }

    if ($Record.configuration.requested_renderer -ne "auto") {
        throw "requested renderer mismatch for GPU probe '$Probe'"
    }
    if ($Record.configuration.requested_gpu_backend -ne $Probe) {
        throw "requested GPU backend mismatch: requested '$Probe', recorded '$($Record.configuration.requested_gpu_backend)'"
    }
    if ($Record.renderer.final -ne "gpu") {
        throw "final renderer mismatch: GPU probe '$Probe' observed '$($Record.renderer.final)'"
    }
    if ($Record.renderer.backend -ne $Probe) {
        throw "actual GPU backend mismatch: requested '$Probe', observed '$($Record.renderer.backend)'"
    }
}

function Invoke-GpuBackendMemoryRun(
    [string] $Probe,
    [AllowNull()]
    [string] $RawPath
) {
    $launcherArguments = @(
        "--app", $app,
        "--scenario", "empty-window",
        "--stabilization-ms", "5000",
        "--sample-interval-ms", "100",
        "--sample-count", "10",
        "--cols", "80",
        "--rows", "24",
        "--json"
    )
    if ($Probe -eq "cpu") {
        $launcherArguments += @("--renderer", "cpu")
    } else {
        $launcherArguments += @("--renderer", "auto", "--gpu-backend", $Probe)
    }

    $jsonLines = @(& $launcher @launcherArguments)
    $exitCode = $LASTEXITCODE
    $json = $jsonLines -join [Environment]::NewLine
    try {
        $record = $json | ConvertFrom-Json
    }
    catch {
        throw "launcher output was not valid JSON (exit code $exitCode)"
    }
    if (-not [string]::IsNullOrWhiteSpace($RawPath)) {
        $record | ConvertTo-Json -Depth 20 -Compress | Set-Content -LiteralPath $RawPath -Encoding utf8NoBOM
    }
    Assert-ProbeRecord -Probe $Probe -Record $record -ExitCode $exitCode
    return $record
}

try {
    $probeReports = [System.Collections.Generic.List[object]]::new()
    foreach ($probe in $probes) {
        $records = [System.Collections.Generic.List[object]]::new()
        $probeFailure = $null
        $completedRuns = 0
        try {
            for ($index = 0; $index -lt $Warmups; $index++) {
                $null = Invoke-GpuBackendMemoryRun -Probe $probe -RawPath $null
            }
            for ($index = 0; $index -lt $Samples; $index++) {
                $rawPath = Join-Path $rawDirectory ("{0}-{1:D2}.json" -f $probe, ($index + 1))
                $record = Invoke-GpuBackendMemoryRun -Probe $probe -RawPath $rawPath
                $records.Add($record)
                $completedRuns++
            }
        }
        catch {
            $probeFailure = Get-SafeFailureMessage $_
        }

        $requestedRenderer = if ($probe -eq "cpu") { "cpu" } else { "auto" }
        if ($null -ne $probeFailure) {
            $failedReport = [ordered]@{
                name = $probe
                status = "failed"
                requested_renderer = $requestedRenderer
                measured_runs_completed = $completedRuns
                samples_per_run = 10
                probe_failure = [ordered]@{
                    message = $probeFailure
                }
            }
            if ($probe -ne "cpu") {
                $failedReport["requested_gpu_backend"] = $probe
            }
            $probeReports.Add($failedReport)
            Write-Warning "GPU backend memory probe '$probe' failed: $probeFailure"
            continue
        }

        [UInt64[]] $bytes = @(
            $records |
                ForEach-Object { $_.memory.samples } |
                ForEach-Object { [UInt64] $_.bytes }
        )
        $p50 = Get-NearestRankPercentile -Values $bytes -Percentile 0.50
        $p95 = Get-NearestRankPercentile -Values $bytes -Percentile 0.95
        $maximum = [UInt64] ($bytes | Measure-Object -Maximum).Maximum
        $firstRecord = $records[0]
        $successfulReport = [ordered]@{
            name = $probe
            status = "succeeded"
            requested_renderer = $requestedRenderer
            final_renderer = $firstRecord.renderer.final
            measured_runs = $Samples
            samples_per_run = 10
            memory_metric = $firstRecord.memory.metric
            memory_p50_bytes = $p50
            memory_p95_bytes = $p95
            memory_max_bytes = $maximum
            report_only_target_bytes = $report_only_target_bytes
            report_only_target_met = ($p95 -le $report_only_target_bytes)
            evidence = [ordered]@{
                raw_pattern = "raw/$probe-NN.json"
            }
        }
        if ($probe -ne "cpu") {
            $successfulReport["requested_gpu_backend"] = $probe
            $successfulReport["actual_gpu_backend"] = $firstRecord.renderer.backend
        }
        $probeReports.Add($successfulReport)
        if ($p95 -gt $report_only_target_bytes) {
            Write-Warning "Report-only memory observation: $probe p95=$p95 target=$report_only_target_bytes"
        }
    }

    $aggregate = [ordered]@{
        schema = "rssh.diagnostics/gpu-backend-memory-matrix-v1"
        profile = $Profile
        warmups = $Warmups
        measured_runs = $Samples
        geometry = [ordered]@{
            columns = 80
            rows = 24
            benchmark_scale_factor = 1.0
        }
        sampling = [ordered]@{
            stabilization_ms = 5000
            interval_ms = 100
            count_per_run = 10
        }
        memory = [ordered]@{
            unit = "bytes"
            report_only_target_bytes = $report_only_target_bytes
        }
        thresholds = "report-only"
        probes = @($probeReports)
        evidence = [ordered]@{
            raw_directory = "raw"
            aggregate = "aggregate.json"
        }
    }
    $aggregate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $aggregatePath -Encoding utf8NoBOM
    $aggregate | ConvertTo-Json -Depth 20 -Compress
}
finally {
    if ($null -eq $previousScale) {
        Remove-Item Env:RSSH_BENCHMARK_WINDOW_SCALE_FACTOR -ErrorAction SilentlyContinue
    } else {
        $env:RSSH_BENCHMARK_WINDOW_SCALE_FACTOR = $previousScale
    }
}
