[CmdletBinding(SupportsShouldProcess)]
param(
    [ValidateSet("debug", "release")]
    [string] $Profile = "release",
    [ValidateRange(0, 100)]
    [int] $Warmups = 5,
    [ValidateRange(1, 1000)]
    [int] $Samples = 30,
    [ValidateRange(1, 60)]
    [int] $ProcessTimeoutSeconds = 60,
    [string] $OutputDirectory = "artifacts/stage7-attribution-matrix",
    [switch] $SkipBuild,
    [string] $AppPath,
    [string] $LauncherPath,
    [string] $RunnerFingerprintPath,
    [string] $TestRunnerFingerprintInputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false

$stages = @(
    "cpu-window",
    "instance-surface",
    "adapter-device",
    "configured-surface-clear",
    "layer-pipelines",
    "fixture-font-text",
    "platform-font-index",
    "full-frame"
)
$backends = @("auto", "dx12", "vulkan", "gl")
$samplesPerProcess = 10
$ownerReadyMarker = "attribution_stage_ready"
$resourceSummarySchema = "rssh.project-owned-resources/v1"
$resourceFieldNames = @(
    "cpu_staging_bytes",
    "cpu_surface_count",
    "cpu_present_count",
    "instance_count",
    "surface_count",
    "adapter_count",
    "device_count",
    "queue_count",
    "surface_configure_count",
    "surface_acquire_count",
    "clear_present_count",
    "pipeline_count",
    "pipeline_layout_count",
    "materialized_buffer_count",
    "retained_font_bytes",
    "inactive_font_bytes",
    "indexed_font_count",
    "active_font_count",
    "catalog_builds",
    "catalog_generation",
    "glyph_atlas_bytes",
    "raster_cache_bytes",
    "image_texture_bytes",
    "snapshot_bytes",
    "instance_buffer_bytes",
    "upload_buffer_bytes",
    "total_allocated_buffer_bytes",
    "total_allocated_texture_bytes",
    "base_text_renderer_materialization_count",
    "cursor_text_renderer_materialization_count",
    "config_load_count",
    "config_watcher_count",
    "pty_start_count",
    "ssh_start_count",
    "post_ready_task_count"
)

function New-InterleavedSchedule([int] $Rounds, [string] $Phase) {
    $schedule = [System.Collections.Generic.List[object]]::new()
    for ($round = 1; $round -le $Rounds; $round++) {
        foreach ($backend in $backends) {
            foreach ($stage in $stages) {
                $schedule.Add([ordered]@{
                    phase = $Phase
                    round = $round
                    backend = $backend
                    stage = $stage
                    requested_renderer = "auto"
                    attribution_stage = $stage
                    stabilization_ms = 5000
                    sample_interval_ms = 100
                    samples_per_process = $samplesPerProcess
                })
            }
        }
    }
    return @($schedule)
}

$plan = [ordered]@{
    schema = "rssh.stage7/attribution-matrix-plan/v1"
    renderer = "auto"
    explicit_backend_override = $false
    stages = $stages
    backends = $backends
    warmups = $Warmups
    measured_cold_processes = $Samples
    samples_per_process = $samplesPerProcess
    stabilization_ms = 5000
    sample_interval_ms = 100
    process_timeout_seconds = $ProcessTimeoutSeconds
    owner_ready_marker = $ownerReadyMarker
    resource_summary_schema = $resourceSummarySchema
    aggregation = [ordered]@{
        process_representative = "nearest-rank-p50"
        cross_process_percentiles = "nearest-rank over per-process representatives"
        maximum = "raw-maximum"
        flattening_for_percentiles = "forbidden"
    }
    atomic_raw_record_files = $backends.Count * $stages.Count * $Samples
    artifacts = @(
        "attribution-matrix-raw",
        "attribution-matrix-aggregate",
        "artifact-manifest-fragment.json"
    )
    schedule = [ordered]@{
        warmups = @(New-InterleavedSchedule -Rounds $Warmups -Phase "warmup")
        measured = @(New-InterleavedSchedule -Rounds $Samples -Phase "measured")
    }
}

if ($WhatIfPreference) {
    $plan | ConvertTo-Json -Depth 30 -Compress
    return
}

if (-not $IsWindows) {
    throw "Stage 7 attribution matrix requires Windows"
}

if (($AppPath -and -not $LauncherPath) -or ($LauncherPath -and -not $AppPath)) {
    throw "both -AppPath and -LauncherPath must be provided together"
}
if (($AppPath -or $LauncherPath) -and -not $SkipBuild) {
    throw "path overrides require -SkipBuild"
}
if ($TestRunnerFingerprintInputPath -and (-not $SkipBuild -or -not $AppPath -or -not $LauncherPath)) {
    throw "test runner fingerprint input requires -SkipBuild with both binary overrides"
}

$certificationEligible = (
    -not $SkipBuild -and
    -not $AppPath -and
    -not $LauncherPath -and
    $Profile -ceq "release" -and
    $Warmups -eq 5 -and
    $Samples -eq 30 -and
    $ProcessTimeoutSeconds -eq 60 -and
    [string]::IsNullOrWhiteSpace($TestRunnerFingerprintInputPath)
)

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "../..")).Path
$profileDirectory = if ($Profile -ceq "release") { "release" } else { "debug" }
$targetRoot = if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
    Join-Path $repoRoot "target"
} elseif ([IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
    [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
} else {
    [IO.Path]::GetFullPath((Join-Path $repoRoot $env:CARGO_TARGET_DIR))
}
$repoBoundary = $repoRoot.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
$targetBoundary = [IO.Path]::GetFullPath($targetRoot).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
$repoPrefix = $repoBoundary + [IO.Path]::DirectorySeparatorChar
if (-not $SkipBuild -and ($targetBoundary.Equals($repoBoundary, [StringComparison]::OrdinalIgnoreCase) -or $targetBoundary.StartsWith($repoPrefix, [StringComparison]::OrdinalIgnoreCase))) {
    throw "CARGO_TARGET_DIR must be outside the repository"
}

if (Test-Path -LiteralPath $OutputDirectory) {
    if (-not (Test-Path -LiteralPath $OutputDirectory -PathType Container)) {
        throw "OutputDirectory must be a directory"
    }
    if ($null -ne (Get-ChildItem -LiteralPath $OutputDirectory -Force | Select-Object -First 1)) {
        throw "OutputDirectory must be empty before collection"
    }
} else {
    New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
}
$outputRoot = (Resolve-Path -LiteralPath $OutputDirectory).Path
$rawRoot = Join-Path $outputRoot "raw"
New-Item -ItemType Directory -Force -Path $rawRoot | Out-Null

$app = if ($AppPath) {
    if (-not (Test-Path -LiteralPath $AppPath -PathType Leaf)) { throw "-AppPath must identify an existing file" }
    (Resolve-Path -LiteralPath $AppPath).Path
} else {
    Join-Path (Join-Path $targetRoot $profileDirectory) "rssh-app.exe"
}
$launcher = if ($LauncherPath) {
    if (-not (Test-Path -LiteralPath $LauncherPath -PathType Leaf)) { throw "-LauncherPath must identify an existing file" }
    (Resolve-Path -LiteralPath $LauncherPath).Path
} else {
    Join-Path (Join-Path $targetRoot $profileDirectory) "rssh-bench-launcher.exe"
}

if (-not $SkipBuild) {
    $profileArguments = @()
    if ($Profile -ceq "release") { $profileArguments += "--release" }
    Push-Location $repoRoot
    try {
        cargo build --locked -p rssh-app --no-default-features --features production-gui,diagnostic-tools @profileArguments
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed for diagnostic rssh-app with exit code $LASTEXITCODE" }
        cargo build --locked -p rssh-diagnostics --bin rssh-bench-launcher @profileArguments
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed for rssh-bench-launcher with exit code $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
}
foreach ($binary in @($app, $launcher)) {
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "required diagnostic binary is missing: $binary"
    }
}

function Get-FileSha256([string] $Path) {
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function ConvertTo-CanonicalValue([AllowNull()] [object] $Value) {
    if ($null -eq $Value) { return $null }
    if ($Value -is [System.Collections.IDictionary]) {
        $ordered = [ordered]@{}
        foreach ($key in @($Value.Keys | ForEach-Object { [string] $_ } | Sort-Object)) {
            $ordered[$key] = ConvertTo-CanonicalValue $Value[$key]
        }
        return $ordered
    }
    if ($Value -is [PSCustomObject]) {
        $ordered = [ordered]@{}
        foreach ($property in @($Value.PSObject.Properties.Name | Sort-Object)) {
            $ordered[$property] = ConvertTo-CanonicalValue $Value.$property
        }
        return $ordered
    }
    if ($Value -is [System.Collections.IEnumerable] -and $Value -isnot [string]) {
        return @($Value | ForEach-Object { ConvertTo-CanonicalValue $_ })
    }
    return $Value
}

function Get-CanonicalSha256([object] $Value) {
    $json = ConvertTo-CanonicalValue $Value | ConvertTo-Json -Depth 100 -Compress
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($json))).ToLowerInvariant()
}

function Get-RunnerCanonicalJson([AllowNull()] [object] $Value, [string] $Label = "runner fields") {
    if ($null -eq $Value) { throw "$Label contains null outside the runner canonical protocol" }
    if ($Value -is [System.Collections.IDictionary]) {
        [string[]] $keys = @($Value.Keys | ForEach-Object { [string] $_ })
        [Array]::Sort($keys, [StringComparer]::Ordinal)
        $members = foreach ($key in $keys) {
            $encodedKey = ConvertTo-Json -InputObject $key -Compress -EscapeHandling Default
            $encodedValue = Get-RunnerCanonicalJson $Value[$key] "$Label.$key"
            "$encodedKey`:$encodedValue"
        }
        return "{$($members -join ',')}"
    }
    if ($Value -is [PSCustomObject]) {
        [string[]] $properties = @($Value.PSObject.Properties.Name)
        [Array]::Sort($properties, [StringComparer]::Ordinal)
        $members = foreach ($property in $properties) {
            $encodedKey = ConvertTo-Json -InputObject $property -Compress -EscapeHandling Default
            $encodedValue = Get-RunnerCanonicalJson $Value.$property "$Label.$property"
            "$encodedKey`:$encodedValue"
        }
        return "{$($members -join ',')}"
    }
    if ($Value -is [System.Collections.IEnumerable] -and $Value -isnot [string]) {
        $items = [System.Collections.Generic.List[string]]::new()
        $index = 0
        foreach ($item in $Value) {
            $items.Add((Get-RunnerCanonicalJson $item "$Label[$index]"))
            $index++
        }
        return "[$($items -join ',')]"
    }
    $isInteger = $Value.GetType() -in @([byte], [sbyte], [Int16], [UInt16], [Int32], [UInt32], [Int64], [UInt64])
    if ($Value -isnot [string] -and $Value -isnot [bool] -and -not $isInteger) {
        throw "$Label contains a value outside the integer/bool/string runner canonical protocol"
    }
    return ConvertTo-Json -InputObject $Value -Compress -EscapeHandling Default
}

function Get-RunnerCanonicalSha256([object] $Value) {
    $bytes = [Text.Encoding]::UTF8.GetBytes((Get-RunnerCanonicalJson $Value))
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
}

function Write-AtomicJson([string] $Path, [object] $Value) {
    $directory = Split-Path -Parent $Path
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
    $leaf = Split-Path -Leaf $Path
    $temporary = Join-Path ([IO.Path]::GetTempPath()) ("rssh-stage7-attribution-{0}-{1}.tmp" -f $PID, [Guid]::NewGuid().ToString("N"))
    try {
        $json = $Value | ConvertTo-Json -Depth 100
        [IO.File]::WriteAllText($temporary, $json, [Text.UTF8Encoding]::new($false))
        Move-Item -LiteralPath $temporary -Destination (Join-Path $directory $leaf) -Force
    } finally {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
    }
}

function Get-RequiredUInt64([AllowNull()] [object] $Value, [string] $Field) {
    if ($null -eq $Value) { throw "$Field must be an unsigned JSON integer" }
    $integerTypes = @([byte], [sbyte], [Int16], [UInt16], [Int32], [UInt32], [Int64], [UInt64])
    if ($Value.GetType() -notin $integerTypes) { throw "$Field must be an unsigned JSON integer" }
    try { return [UInt64] $Value } catch { throw "$Field must be an unsigned JSON integer" }
}

function Get-NearestRankPercentile([object[]] $Values, [double] $Percentile) {
    if ($null -eq $Values -or $Values.Count -eq 0) { throw "nearest-rank percentile requires at least one value" }
    [UInt64[]] $ordered = @($Values | ForEach-Object { [UInt64] $_ } | Sort-Object)
    [int] $rank = [Math]::Ceiling($Percentile * $ordered.Count)
    [int] $index = [Math]::Max(0, $rank - 1)
    return [UInt64] $ordered[$index]
}

function Assert-ExactPropertySet([object] $Value, [string[]] $Expected, [string] $Label) {
    if ($null -eq $Value) { throw "$Label is missing" }
    $actual = @($Value.PSObject.Properties.Name | Sort-Object)
    $expected = @($Expected | Sort-Object)
    if ((ConvertTo-Json $actual -Compress) -cne (ConvertTo-Json $expected -Compress)) {
        throw "$Label fields are not closed"
    }
}

function Test-ProductionAdapterType([AllowNull()] [object] $Value) {
    return $Value -is [string] -and $Value -cin @("other", "integrated-gpu", "discrete-gpu", "virtual-gpu", "cpu")
}

function Get-SafeFailureMessage([System.Management.Automation.ErrorRecord] $Failure) {
    $message = $Failure.Exception.Message
    foreach ($path in @($repoRoot, $targetRoot, $outputRoot, $AppPath, $LauncherPath, $app, $launcher)) {
        if (-not [string]::IsNullOrWhiteSpace($path)) { $message = $message.Replace([string] $path, "[path]") }
    }
    return $message
}

function Assert-CollectionIdentityUnchanged {
    $currentAppHash = Get-FileSha256 $app
    $currentLauncherHash = Get-FileSha256 $launcher
    if ($currentAppHash -cne $binaryHashes["rssh-app.exe"] -or $currentLauncherHash -cne $binaryHashes["rssh-bench-launcher.exe"]) {
        throw "diagnostic binary identity changed during collection"
    }
    $currentSource = (& git -C $repoRoot rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $currentSource -cne $sourceSha) { throw "source commit changed during collection" }
    if ($certificationEligible) {
        $dirty = @(& git -C $repoRoot status --porcelain)
        if ($LASTEXITCODE -ne 0 -or $dirty.Count -ne 0) { throw "certification source tree changed during collection including untracked files" }
    }
}

function Get-AllowedResourceFields([string] $Stage) {
    $index = [Array]::IndexOf($stages, $Stage)
    $allowed = @("cpu_staging_bytes", "cpu_surface_count", "cpu_present_count")
    if ($index -ge 1) { $allowed += @("instance_count", "surface_count") }
    if ($index -ge 2) { $allowed += @("adapter_count", "device_count", "queue_count") }
    if ($index -ge 3) { $allowed += @("surface_configure_count", "surface_acquire_count", "clear_present_count") }
    if ($index -ge 4) { $allowed += @("pipeline_count", "pipeline_layout_count", "materialized_buffer_count", "total_allocated_buffer_bytes") }
    if ($index -ge 5) { $allowed += @("retained_font_bytes", "active_font_count", "catalog_builds", "catalog_generation", "glyph_atlas_bytes", "raster_cache_bytes", "instance_buffer_bytes", "upload_buffer_bytes", "total_allocated_texture_bytes", "base_text_renderer_materialization_count", "cursor_text_renderer_materialization_count") }
    if ($index -ge 6) { $allowed += "indexed_font_count" }
    if ($index -ge 7) { $allowed += @("image_texture_bytes", "snapshot_bytes") }
    return $allowed
}

function Assert-ProjectOwnedResourceMetricsV1([object] $Summary, [string] $Stage, [string] $ExpectedBackend, [string] $ExpectedAdapterName) {
    # This is the PowerShell boundary validator for ProjectOwnedResourceMetricsV1.
    if ($null -eq $Summary) { throw "ProjectOwnedResourceMetricsV1 resource_summary is missing" }
    $allowedProperties = @($resourceFieldNames + "backend" + "adapter_name")
    $actualProperties = @($Summary.PSObject.Properties.Name)
    $unknown = @($actualProperties | Where-Object { $_ -notin $allowedProperties })
    if ($unknown.Count -ne 0) { throw "resource_summary contains unknown fields: $($unknown -join ', ')" }
    foreach ($field in $resourceFieldNames) {
        if ($actualProperties -notcontains $field) { throw "resource_summary is missing $field" }
        $null = Get-RequiredUInt64 $Summary.$field "resource_summary.$field"
    }
    $index = [Array]::IndexOf($stages, $Stage)
    foreach ($field in $resourceFieldNames) {
        [UInt64] $value = Get-RequiredUInt64 $Summary.$field "resource_summary.$field"
        if ($value -ne 0 -and $field -notin (Get-AllowedResourceFields $Stage)) { throw "resource_summary.$field must remain zero at $Stage" }
    }
    if ((Get-RequiredUInt64 $Summary.cpu_staging_bytes "resource_summary.cpu_staging_bytes") -eq 0) { throw "resource_summary.cpu_staging_bytes must be positive" }
    foreach ($pair in @(
        @("cpu_surface_count", 1), @("cpu_present_count", 1)
    )) {
        if ((Get-RequiredUInt64 $Summary.$($pair[0]) "resource_summary.$($pair[0])") -ne [UInt64] $pair[1]) { throw "resource_summary.$($pair[0]) must be $($pair[1])" }
    }
    if ($index -ge 1) {
        foreach ($field in @("instance_count", "surface_count")) { if ((Get-RequiredUInt64 $Summary.$field "resource_summary.$field") -ne 1) { throw "resource_summary.$field must be one" } }
    }
    if ($index -ge 2) {
        foreach ($field in @("adapter_count", "device_count", "queue_count")) { if ((Get-RequiredUInt64 $Summary.$field "resource_summary.$field") -ne 1) { throw "resource_summary.$field must be one" } }
        if ($actualProperties -notcontains "backend" -or $Summary.backend -notin @("dx12", "vulkan", "gl")) { throw "resource_summary.backend is required from adapter-device onward" }
        if ($actualProperties -notcontains "adapter_name" -or $Summary.adapter_name -isnot [string] -or [string]::IsNullOrWhiteSpace($Summary.adapter_name)) { throw "resource_summary.adapter_name is required from adapter-device onward" }
        if ($Summary.backend -cne $ExpectedBackend -or $Summary.adapter_name -cne $ExpectedAdapterName) { throw "resource_summary backend/adapter identity differs from renderer" }
    } elseif ($actualProperties -contains "backend" -or $actualProperties -contains "adapter_name") {
        throw "resource_summary backend and adapter_name must be absent before adapter-device"
    }
    if ($index -ge 3) {
        if ((Get-RequiredUInt64 $Summary.surface_configure_count "resource_summary.surface_configure_count") -ne 1 -or (Get-RequiredUInt64 $Summary.clear_present_count "resource_summary.clear_present_count") -ne 1) { throw "configured-surface-clear resource counts are invalid" }
        $expectedAcquires = if ($index -ge 7) { 3 } elseif ($index -ge 5) { 2 } else { 1 }
        if ((Get-RequiredUInt64 $Summary.surface_acquire_count "resource_summary.surface_acquire_count") -ne $expectedAcquires) { throw "resource_summary.surface_acquire_count is invalid at $Stage" }
    }
    if ($index -ge 4) {
        foreach ($pair in @(@("pipeline_count", 2), @("pipeline_layout_count", 2), @("materialized_buffer_count", 1))) { if ((Get-RequiredUInt64 $Summary.$($pair[0]) "resource_summary.$($pair[0])") -ne [UInt64] $pair[1]) { throw "resource_summary.$($pair[0]) is invalid" } }
        if ((Get-RequiredUInt64 $Summary.total_allocated_buffer_bytes "resource_summary.total_allocated_buffer_bytes") -eq 0) { throw "resource_summary.total_allocated_buffer_bytes must be positive" }
    }
    if ($index -ge 5) {
        foreach ($field in @("retained_font_bytes", "active_font_count", "catalog_builds", "catalog_generation", "glyph_atlas_bytes")) { if ((Get-RequiredUInt64 $Summary.$field "resource_summary.$field") -eq 0) { throw "resource_summary.$field must be positive" } }
        $texture = (Get-RequiredUInt64 $Summary.glyph_atlas_bytes "resource_summary.glyph_atlas_bytes") + (Get-RequiredUInt64 $Summary.image_texture_bytes "resource_summary.image_texture_bytes")
        if ((Get-RequiredUInt64 $Summary.total_allocated_texture_bytes "resource_summary.total_allocated_texture_bytes") -ne $texture) { throw "resource_summary texture total is inconsistent" }
        $textCount = if ($index -ge 7) { 2 } else { 1 }
        foreach ($field in @("base_text_renderer_materialization_count", "cursor_text_renderer_materialization_count")) { if ((Get-RequiredUInt64 $Summary.$field "resource_summary.$field") -ne $textCount) { throw "resource_summary.$field is invalid" } }
    }
    if ($index -ge 6 -and (Get-RequiredUInt64 $Summary.indexed_font_count "resource_summary.indexed_font_count") -eq 0) { throw "resource_summary.indexed_font_count must be positive" }
    if ($index -ge 6 -and (Get-RequiredUInt64 $Summary.inactive_font_bytes "resource_summary.inactive_font_bytes") -ne 0) { throw "resource_summary.inactive_font_bytes must remain zero at platform-font-index" }
    if ($index -ge 7 -and (Get-RequiredUInt64 $Summary.snapshot_bytes "resource_summary.snapshot_bytes") -eq 0) { throw "resource_summary.snapshot_bytes must be positive" }
}

function Assert-StageRecord([string] $Backend, [string] $Stage, [object] $Record) {
    if ($Record.schema -cne "rssh.diagnostics/v2" -or $Record.readiness.status -cne "ready" -or $Record.failures.Count -ne 0) { throw "diagnostic did not produce a ready result" }
    if ($Record.run.id -isnot [string] -or [string]::IsNullOrWhiteSpace($Record.run.id)) { throw "diagnostic did not retain an actual run ID" }
    foreach ($field in @(@("stabilization_ms", 5000), @("sample_interval_ms", 100), @("sample_count", 10), @("columns", 80), @("rows", 24), @("scale_factor_milli", 1000))) {
        if ((Get-RequiredUInt64 $Record.configuration.$($field[0]) "configuration.$($field[0])") -ne [UInt64] $field[1]) { throw "configuration.$($field[0]) mismatch" }
    }
    if ($Record.configuration.requested_renderer -cne "auto" -or $Record.configuration.requested_attribution_stage -cne $Stage) { throw "requested attribution configuration mismatch" }
    if ($Backend -eq "auto") {
        if ($Record.configuration.PSObject.Properties.Name -contains "requested_gpu_backend") { throw "auto product run unexpectedly requested a backend" }
    } elseif ($Record.configuration.requested_gpu_backend -cne $Backend) {
        throw "requested GPU backend mismatch"
    }
    $stageIndex = [Array]::IndexOf($stages, $Stage)
    $expectedRenderer = if ($stageIndex -ge 3) { "gpu" } else { "cpu" }
    if ($Record.renderer.final -cne $expectedRenderer) { throw "attribution stage final renderer mismatch" }
    if ($Record.final_attribution_stage -cne $Stage -or $Record.resource_summary_schema -cne $resourceSummarySchema) { throw "owner attribution stage/schema mismatch" }
    if (-not @($Record.readiness.evidence | ForEach-Object { [string] $_ } | Where-Object { $_ -match "attribution_stage_ready" })) { throw "exact owner-produced attribution_stage_ready evidence is missing" }
    if ($Record.memory.metric -cne "windows_private_working_set_bytes" -or $Record.memory.unit -cne "bytes" -or $Record.memory.samples.Count -ne $samplesPerProcess) { throw "memory sample schema mismatch" }
    $samples = [System.Collections.Generic.List[UInt64]]::new()
    for ($index = 0; $index -lt $samplesPerProcess; $index++) {
        $sample = $Record.memory.samples[$index]
        if ((Get-RequiredUInt64 $sample.sequence "memory.samples[$index].sequence") -ne [UInt64] $index) { throw "memory sample sequence mismatch" }
        $samples.Add((Get-RequiredUInt64 $sample.bytes "memory.samples[$index].bytes"))
    }
    if ($stageIndex -ge 2) {
        if ($Record.renderer.backend -notin @("dx12", "vulkan", "gl") -or $Record.renderer.adapter_name -isnot [string] -or [string]::IsNullOrWhiteSpace($Record.renderer.adapter_name) -or -not (Test-ProductionAdapterType $Record.renderer.adapter_type)) { throw "GPU adapter identity is missing or invalid" }
        $expectedBackend = if ($Backend -eq "auto") { [string] $Record.renderer.backend } else { $Backend }
        Assert-ProjectOwnedResourceMetricsV1 -Summary $Record.resource_summary -Stage $Stage -ExpectedBackend $expectedBackend -ExpectedAdapterName ([string] $Record.renderer.adapter_name)
    } else {
        Assert-ProjectOwnedResourceMetricsV1 -Summary $Record.resource_summary -Stage $Stage -ExpectedBackend "" -ExpectedAdapterName ""
    }
    return @($samples)
}

function Invoke-StageDiagnostic([string] $Backend, [string] $Stage) {
    $arguments = @(
        "--app", $app,
        "--scenario", "empty-window",
        "--stabilization-ms", "5000",
        "--sample-interval-ms", "100",
        "--sample-count", "10",
        "--cols", "80",
        "--rows", "24",
        "--renderer", "auto",
        "--attribution-stage", $Stage,
        "--json"
    )
    if ($Backend -ne "auto") { $arguments += @("--gpu-backend", $Backend) }
    $boundedFile = $launcher
    $boundedArguments = $arguments
    if ([IO.Path]::GetExtension($launcher) -cin @(".cmd", ".bat")) {
        $boundedFile = "cmd.exe"
        $boundedArguments = @("/D", "/S", "/C", $launcher) + $arguments
    }
    try {
        $bounded = Invoke-BoundedProcess -Phase "Stage 7 attribution $Backend/$Stage" -FilePath $boundedFile -ArgumentList $boundedArguments -TimeoutSeconds $ProcessTimeoutSeconds 6>$null
        Assert-CollectionIdentityUnchanged
        try { $record = $bounded.Stdout.Trim() | ConvertFrom-Json } catch { throw "launcher output was not valid JSON" }
        $samples = Assert-StageRecord -Backend $Backend -Stage $Stage -Record $record
        return [pscustomobject]@{ status = "succeeded"; backend = $Backend; stage = $Stage; record = $record; samples = $samples; failure_classification = $null; failure_message = $null }
    } catch {
        $message = Get-SafeFailureMessage $_
        $classification = if ($message -match "(?i)unsupported|not available|no adapter|request device|backend") { "backend-unsupported" } elseif ($message -match "(?i)deadline|timeout|exceeded") { "timeout" } elseif ($message -match "identity drift") { "identity-drift" } else { "stage-contract" }
        return [pscustomobject]@{ status = "failed"; backend = $Backend; stage = $Stage; record = $null; samples = @(); failure_classification = $classification; failure_message = $message }
    }
}

function Assert-RunnerObservation([object] $Observation) {
    if ($Observation.schema -cne "rssh.stage7/runner-fingerprint/v1" -or $Observation.complete -ne $true -or $Observation.source -notin @("host-probe", "fixture")) { throw "runner fingerprint observation is incomplete" }
    Assert-ExactPropertySet -Value $Observation.fields -Expected @("os", "gpu", "memory", "displays", "power_plan", "session", "locale", "fonts", "cold_cache_policy") -Label "runner fingerprint fields"
    if ($Observation.fields.fonts.inventory_fingerprint_sha256 -cnotmatch "^[0-9a-f]{64}$" -or (Get-RequiredUInt64 $Observation.fields.fonts.index_policy_version "runner font index policy version") -eq 0) { throw "runner font inventory identity is invalid" }
    if ($Observation.fields.cold_cache_policy.process_cold_start -ne $true -or $Observation.fields.cold_cache_policy.os_file_cache -cne "unmodified-no-explicit-flush") { throw "runner cold-cache policy is invalid" }
    if ($Observation.fingerprint_sha256 -cne (Get-RunnerCanonicalSha256 $Observation.fields)) { throw "runner fingerprint digest is not canonical" }
    $collectorPath = Join-Path $PSScriptRoot "collect-stage7-runner-fingerprint.ps1"
    if ($Observation.collector_script_sha256 -cne (Get-FileSha256 $collectorPath)) { throw "runner fingerprint collector identity drifted" }
    if ((Get-RequiredUInt64 $Observation.collector_timeout_seconds "collector timeout") -ne 60) { throw "runner fingerprint collector timeout must be 60 seconds" }
}

function Get-RunnerObservation([object] $FirstAdapterRecord) {
    $candidate = $RunnerFingerprintPath
    if ([string]::IsNullOrWhiteSpace($candidate)) {
        $candidate = Join-Path (Split-Path -Parent $outputRoot) "font/runner-fingerprint.json"
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { $candidate = $null }
    }
    if ($candidate) {
        $payload = Get-Content -LiteralPath $candidate -Raw | ConvertFrom-Json
        if ($payload.schema -cne "rssh.stage7/runner-fingerprint/v1") {
            $payload = [ordered]@{
                schema = "rssh.stage7/runner-fingerprint/v1"
                source = if ($payload.source) { $payload.source } else { "fixture" }
                complete = $payload.complete
                fields = $payload.fields
                fingerprint_sha256 = $payload.fingerprint_sha256
                collector_script_sha256 = $payload.collector_script_sha256
                collector_timeout_seconds = $payload.collector_timeout_seconds
            }
        }
        Assert-RunnerObservation $payload
        return $payload
    }

    if ($null -eq $FirstAdapterRecord) { throw "a successful auto adapter-device run is required before collecting runner fingerprint" }
    if ($env:RSSH_STAGE7_FONT_INVENTORY_FINGERPRINT -cnotmatch "^[0-9a-f]{64}$" -or $env:RSSH_STAGE7_FONT_INVENTORY_FINGERPRINT -ceq ("0" * 64)) {
        throw "runner fingerprint requires the Stage 7 font cohort fingerprint or a runner input artifact"
    }
    $inventory = $env:RSSH_STAGE7_FONT_INVENTORY_FINGERPRINT
    $fontPolicy = if ($env:RSSH_STAGE7_FONT_INDEX_POLICY_VERSION) { $env:RSSH_STAGE7_FONT_INDEX_POLICY_VERSION } else { "1" }
    . (Join-Path $PSScriptRoot "process-harness.ps1")
    $collectorArgs = @(
        "-NoProfile", "-NonInteractive", "-File", (Join-Path $PSScriptRoot "collect-stage7-runner-fingerprint.ps1"),
        "-GpuVendorId", ([string] (Get-RequiredUInt64 $FirstAdapterRecord.renderer.adapter_vendor_id "renderer.adapter_vendor_id")),
        "-GpuDeviceId", ([string] (Get-RequiredUInt64 $FirstAdapterRecord.renderer.adapter_device_id "renderer.adapter_device_id")),
        "-GpuAdapterName", ([string] $FirstAdapterRecord.renderer.adapter_name),
        "-FontInventoryFingerprintSha256", $inventory,
        "-FontIndexPolicyVersion", ([string] $fontPolicy)
    )
    $bounded = Invoke-BoundedProcess -Phase "Stage 7 runner fingerprint collector" -FilePath "pwsh.exe" -ArgumentList $collectorArgs -TimeoutSeconds 60 6>$null
    if (-not [string]::IsNullOrWhiteSpace($bounded.Stderr)) { throw "runner fingerprint collector stderr must be empty" }
    $observation = $bounded.Stdout.Trim() | ConvertFrom-Json
    Assert-RunnerObservation $observation
    return $observation
}

$previousScale = $env:RSSH_BENCHMARK_WINDOW_SCALE_FACTOR
$env:RSSH_BENCHMARK_WINDOW_SCALE_FACTOR = "1"
$sourceSha = (& git -C $repoRoot rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $sourceSha -cnotmatch "^[0-9a-f]{40}$") { throw "unable to resolve one immutable source commit" }
if ($certificationEligible) {
    $dirty = @(& git -C $repoRoot status --porcelain)
    if ($LASTEXITCODE -ne 0 -or $dirty.Count -ne 0) { throw "certification collection requires a clean source tree including untracked files" }
}
$sourceTreeSha = (& git -C $repoRoot rev-parse "HEAD^{tree}").Trim()
$binaryHashes = [ordered]@{
    "rssh-app.exe" = Get-FileSha256 $app
    "rssh-bench-launcher.exe" = Get-FileSha256 $launcher
}
$collectionRunId = "stage7-attribution-{0}-{1}" -f ([DateTimeOffset]::UtcNow.ToUnixTimeSeconds()), $PID
$warmupIds = [System.Collections.Generic.List[string]]::new()
$warmupSet = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$cellStates = @{}
$identityByCell = @{}
$failures = [System.Collections.Generic.List[object]]::new()
$firstAutoAdapterRecord = $null
$fatalProductFailure = $false

# Use the same suspended CreateProcessW/job-object harness as the font proof;
# every matrix cell must be a bounded, fresh process.
. (Join-Path $PSScriptRoot "process-harness.ps1")

for ($round = 1; $round -le $Warmups; $round++) {
    foreach ($backend in $backends) {
        foreach ($stage in $stages) {
            $key = "$backend/$stage"
            if ($cellStates[$key] -eq "unsupported" -or $cellStates[$key] -eq "failed") { continue }
            $outcome = Invoke-StageDiagnostic -Backend $backend -Stage $stage
            if ($outcome.status -eq "succeeded") {
                $id = [string] $outcome.record.run.id
                if ($warmupSet.Add($id) -and $warmupIds.Count -lt 5) { $warmupIds.Add($id) }
                if ($backend -eq "auto" -and $stage -eq "adapter-device" -and $null -eq $firstAutoAdapterRecord) { $firstAutoAdapterRecord = $outcome.record }
                $stageIndex = [Array]::IndexOf($stages, $stage)
                if ($stageIndex -ge 2) {
                    $identity = [ordered]@{
                        actual_backend = [string] $outcome.record.renderer.backend
                        adapter_name = [string] $outcome.record.renderer.adapter_name
                        adapter_vendor_id = Get-RequiredUInt64 $outcome.record.renderer.adapter_vendor_id "renderer.adapter_vendor_id"
                        adapter_device_id = Get-RequiredUInt64 $outcome.record.renderer.adapter_device_id "renderer.adapter_device_id"
                        adapter_type = [string] $outcome.record.renderer.adapter_type
                    }
                    $identityDigest = Get-CanonicalSha256 $identity
                    if ($identityByCell.ContainsKey($key) -and $identityByCell[$key].digest -cne $identityDigest) {
                        $cellStates[$key] = "failed"
                        $fatalProductFailure = $true
                        $failures.Add([ordered]@{ backend = $backend; stage = $stage; classification = "identity-drift"; message = "GPU identity drift across cold runs" })
                    } else { $identityByCell[$key] = [pscustomobject]@{ digest = $identityDigest; details = $identity } }
                }
            } else {
                $failures.Add([ordered]@{ backend = $backend; stage = $stage; classification = $outcome.failure_classification; message = $outcome.failure_message })
                if ($backend -ne "auto" -and $outcome.failure_classification -eq "backend-unsupported") {
                    $cellStates[$key] = "unsupported"
                    $cellStates["$backend/__unsupported_at"] = $stage
                    $cellStates["$backend/__unsupported_reason"] = "backend-unavailable"
                } else {
                    $cellStates[$key] = "failed"
                    if ($backend -eq "auto") { $fatalProductFailure = $true }
                }
            }
        }
    }
}
while ($warmupIds.Count -lt 5) {
    $synthetic = "stage7-attribution-warmup-{0:D2}" -f ($warmupIds.Count + 1)
    if ($warmupSet.Add($synthetic)) { $warmupIds.Add($synthetic) }
}

$runnerObservation = $null
try {
    $runnerObservation = Get-RunnerObservation -FirstAdapterRecord $firstAutoAdapterRecord
    if ($certificationEligible -and $runnerObservation.source -cne "host-probe") {
        $certificationEligible = $false
    }
} catch {
    $message = Get-SafeFailureMessage $_
    $failures.Add([ordered]@{ backend = "runner"; stage = "fingerprint"; classification = "runner-fingerprint"; message = $message })
    $fatalProductFailure = $true
}

$identity = [ordered]@{
    source_sha = $sourceSha
    binary_hashes = $binaryHashes
    runner_fingerprint_sha256 = if ($runnerObservation) { [string] $runnerObservation.fingerprint_sha256 } else { "0" * 64 }
    platform = "windows-x86_64"
    run_id = $collectionRunId
}
$measured = @{}
foreach ($backend in $backends) { foreach ($stage in $stages) { $measured["$backend/$stage"] = [System.Collections.Generic.List[object]]::new() } }

for ($round = 1; $round -le $Samples; $round++) {
    foreach ($backend in $backends) {
        foreach ($stage in $stages) {
            $key = "$backend/$stage"
            if ($cellStates[$key] -eq "unsupported" -or $cellStates[$key] -eq "failed") { continue }
            $outcome = Invoke-StageDiagnostic -Backend $backend -Stage $stage
            if ($outcome.status -ne "succeeded") {
                $failures.Add([ordered]@{ backend = $backend; stage = $stage; round = $round; classification = $outcome.failure_classification; message = $outcome.failure_message })
                if ($backend -ne "auto" -and $outcome.failure_classification -eq "backend-unsupported") {
                    $cellStates[$key] = "unsupported"
                    $cellStates["$backend/__unsupported_at"] = $stage
                    $cellStates["$backend/__unsupported_reason"] = "backend-unavailable"
                } else {
                    $cellStates[$key] = "failed"
                    if ($backend -eq "auto") { $fatalProductFailure = $true }
                }
                continue
            }
            $stageIndex = [Array]::IndexOf($stages, $stage)
            if ($stageIndex -ge 2) {
                $identityValue = [ordered]@{
                    actual_backend = [string] $outcome.record.renderer.backend
                    adapter_name = [string] $outcome.record.renderer.adapter_name
                    adapter_vendor_id = Get-RequiredUInt64 $outcome.record.renderer.adapter_vendor_id "renderer.adapter_vendor_id"
                    adapter_device_id = Get-RequiredUInt64 $outcome.record.renderer.adapter_device_id "renderer.adapter_device_id"
                    adapter_type = [string] $outcome.record.renderer.adapter_type
                }
                $digest = Get-CanonicalSha256 $identityValue
                if ($identityByCell.ContainsKey($key) -and $identityByCell[$key].digest -cne $digest) {
                    $cellStates[$key] = "failed"
                    $fatalProductFailure = $true
                    $failures.Add([ordered]@{ backend = $backend; stage = $stage; round = $round; classification = "identity-drift"; message = "GPU identity drift across cold runs" })
                    continue
                }
            }
            $measured[$key].Add([ordered]@{
                process_id = [string] $outcome.record.run.id
                phase = "measured"
                round_index = $round
                samples = @($outcome.samples)
                representative = Get-NearestRankPercentile -Values @($outcome.samples) -Percentile 0.50
                attribution_stage = $stage
                resource_summary_schema = $resourceSummarySchema
                resource_summary = $outcome.record.resource_summary
            })
        }
    }
}

function New-Statistics([object[]] $Processes) {
    $representatives = @($Processes | ForEach-Object { [UInt64] $_.representative })
    $raw = @($Processes | ForEach-Object { $_.samples } | ForEach-Object { [UInt64] $_ })
    return [ordered]@{
        p50 = Get-NearestRankPercentile -Values $representatives -Percentile 0.50
        p95 = Get-NearestRankPercentile -Values $representatives -Percentile 0.95
        max = [UInt64] ($raw | Measure-Object -Maximum).Maximum
    }
}

$rawChildren = [System.Collections.Generic.List[string]]::new()
$groupStatistics = [System.Collections.Generic.List[object]]::new()
$representativeReport = [System.Collections.Generic.List[object]]::new()
$rawMaximaReport = [System.Collections.Generic.List[object]]::new()
$identityReport = [System.Collections.Generic.List[object]]::new()
$unsupportedGroups = @{}

foreach ($backend in $backends) {
    $unsupportedAt = $cellStates["$backend/__unsupported_at"]
    $unsupportedReason = $cellStates["$backend/__unsupported_reason"]
    foreach ($stage in $stages) {
        $key = "$backend/$stage"
        $stageIndex = [Array]::IndexOf($stages, $stage)
        if ($backend -ne "auto" -and $unsupportedAt -and $stageIndex -ge [Array]::IndexOf($stages, $unsupportedAt)) {
            $unsupportedGroups[$key] = [ordered]@{ reason = $unsupportedReason; at = $unsupportedAt }
            continue
        }
        if ($cellStates[$key] -eq "failed" -or $measured[$key].Count -ne $Samples) {
            if ($measured[$key].Count -ne $Samples) { $failures.Add([ordered]@{ backend = $backend; stage = $stage; classification = "incomplete-cohort"; message = "measured cold cohort did not contain exactly $Samples processes" }) }
            $fatalProductFailure = $true
            continue
        }
        $processes = @($measured[$key])
        $stats = New-Statistics -Processes $processes
        $groupStatistics.Add($stats)
        $representativeReport.Add([ordered]@{ name = $key; values = @($processes | ForEach-Object { [UInt64] $_.representative }) })
        $rawMaximaReport.Add([ordered]@{ name = $key; bytes = [UInt64] $stats.max })
        $stageIdentity = if ($identityByCell.ContainsKey($key)) { $identityByCell[$key].details } else { $null }
        $identityReport.Add([ordered]@{ name = $key; status = "supported"; actual_backend = if ($stageIdentity) { $stageIdentity.actual_backend } else { $null }; adapter_identity = if ($stageIdentity) { $identityByCell[$key].digest } else { $null } })
        for ($index = 0; $index -lt $processes.Count; $index++) {
            $process = $processes[$index]
            $rawId = "attribution-matrix-raw/$backend/$stage/process-{0:D3}" -f ($index + 1)
            $relativePath = "raw/{0}-{1}-process-{2:D3}.json" -f $backend, $stage, ($index + 1)
            $group = [ordered]@{
                name = $key
                metric = "windows_private_working_set_bytes"
                sampling_mode = "residence"
                requested_backend = $backend
                final_renderer = "gpu"
                support_status = "supported"
                owner_ready_marker = $ownerReadyMarker
                stabilization_ms = 5000
                sample_interval_ms = 100
                processes = @($process)
            }
            if ($stageIndex -ge 2) {
                $details = $identityByCell[$key].details
                $group.actual_backend = $details.actual_backend
                $group.adapter_identity = $identityByCell[$key].digest
            }
            $rawPayload = [ordered]@{
                schema = "rssh.stage7.metric-raw/v1"
                certification_eligible = $certificationEligible
                identity = $identity
                warmups = $Warmups
                warmup_process_ids = @($warmupIds)
                measured_cold_processes = $Samples
                timeout_seconds = $ProcessTimeoutSeconds
                protocol = [ordered]@{
                    warmups = $Warmups
                    measured_cold_processes = $Samples
                    timeout_seconds = $ProcessTimeoutSeconds
                    cross_process_percentiles = "nearest-rank"
                    maximum = "raw-maximum"
                    sampling_mode = "residence"
                    samples_per_process = 10
                    stabilization_ms = 5000
                    sample_interval_ms = 100
                    process_representative = "nearest-rank-p50"
                    flattening_for_percentiles = "forbidden"
                    owner_ready_marker = $ownerReadyMarker
                }
                groups = @($group)
            }
            Write-AtomicJson -Path (Join-Path $outputRoot $relativePath) -Value $rawPayload
            $rawChildren.Add($rawId)
        }
    }
}

foreach ($backend in $backends) {
    foreach ($stage in $stages) {
        $key = "$backend/$stage"
        if ($unsupportedGroups.ContainsKey($key)) {
            $rawId = "attribution-matrix-raw/$backend/$stage/unsupported"
            $relativePath = "raw/{0}-{1}-unsupported.json" -f $backend, $stage
            $outcome = $unsupportedGroups[$key]
            $rawPayload = [ordered]@{
                schema = "rssh.stage7.metric-raw/v1"
                certification_eligible = $certificationEligible
                identity = $identity
                warmups = $Warmups
                warmup_process_ids = @($warmupIds)
                measured_cold_processes = $Samples
                timeout_seconds = $ProcessTimeoutSeconds
                protocol = [ordered]@{
                    warmups = $Warmups; measured_cold_processes = $Samples; timeout_seconds = $ProcessTimeoutSeconds; cross_process_percentiles = "nearest-rank"; maximum = "raw-maximum"; sampling_mode = "residence"; samples_per_process = 10; stabilization_ms = 5000; sample_interval_ms = 100; process_representative = "nearest-rank-p50"; flattening_for_percentiles = "forbidden"; owner_ready_marker = $ownerReadyMarker
                }
                groups = @([ordered]@{
                    name = $key
                    metric = "windows_private_working_set_bytes"
                    sampling_mode = "residence"
                    requested_backend = $backend
                    support_status = "unsupported"
                    unsupported_reason = $outcome.reason
                    unsupported_at_stage = $outcome.at
                })
            }
            Write-AtomicJson -Path (Join-Path $outputRoot $relativePath) -Value $rawPayload
            $rawChildren.Add($rawId)
            $identityReport.Add([ordered]@{ name = $key; status = "unsupported"; unsupported_reason = $outcome.reason; unsupported_at_stage = $outcome.at })
        }
    }
}

$adjacentDeltas = [System.Collections.Generic.List[object]]::new()
foreach ($backend in $backends) {
    for ($index = 1; $index -lt $stages.Count; $index++) {
        $fromKey = "$backend/$($stages[$index - 1])"
        $toKey = "$backend/$($stages[$index])"
        if ($unsupportedGroups.ContainsKey($fromKey) -or $unsupportedGroups.ContainsKey($toKey) -or $measured[$fromKey].Count -ne $Samples -or $measured[$toKey].Count -ne $Samples) {
            $adjacentDeltas.Add([ordered]@{ backend = $backend; from_stage = $stages[$index - 1]; to_stage = $stages[$index]; status = "not-applicable" })
            continue
        }
        $fromStats = New-Statistics -Processes @($measured[$fromKey])
        $toStats = New-Statistics -Processes @($measured[$toKey])
        $adjacentDeltas.Add([ordered]@{ backend = $backend; from_stage = $stages[$index - 1]; to_stage = $stages[$index]; status = "report-only"; p50_delta_bytes = [Int64] $toStats.p50 - [Int64] $fromStats.p50; p95_delta_bytes = [Int64] $toStats.p95 - [Int64] $fromStats.p95; max_delta_bytes = [Int64] $toStats.max - [Int64] $fromStats.max })
    }
}

$aggregatePath = Join-Path $outputRoot "attribution-matrix-aggregate.json"
$aggregate = [ordered]@{
    schema = "rssh.stage7.metric-aggregate/v1"
    certification_eligible = $certificationEligible -and -not $fatalProductFailure
    identity = $identity
    ok = -not $fatalProductFailure
    raw_children = @($rawChildren | Sort-Object)
    group_statistics = @($groupStatistics)
    representatives = @($representativeReport)
    raw_maxima = @($rawMaximaReport)
    identities = @($identityReport)
    failure_classifications = @($failures)
    adjacent_stage_deltas = @($adjacentDeltas)
    source_tree_sha256 = $sourceTreeSha
    runner_fingerprint_sha256 = $identity.runner_fingerprint_sha256
}
Write-AtomicJson -Path $aggregatePath -Value $aggregate

$fragmentPath = Join-Path $outputRoot "artifact-manifest-fragment.json"
if (-not $fatalProductFailure) {
    function New-FragmentEntry([string] $ArtifactType, [string] $ArtifactId, [string] $Role, [string] $PayloadSchema, [string] $RelativePath, [string[]] $Children) {
        $entry = [ordered]@{
            artifact_type = $ArtifactType
            artifact_id = $ArtifactId
            certification_eligible = $certificationEligible
            role = $Role
            scope = "attribution-ready"
            payload_schema = $PayloadSchema
            path = $RelativePath
            sha256 = Get-FileSha256 (Join-Path $outputRoot $RelativePath)
            size_bytes = [UInt64] (Get-Item -LiteralPath (Join-Path $outputRoot $RelativePath)).Length
            producing_command = "pwsh -File scripts/ci/run-stage7-attribution-matrix.ps1"
            producing_argv = @("pwsh", "-File", "scripts/ci/run-stage7-attribution-matrix.ps1")
            source_sha = $sourceSha
            subject_refs = [ordered]@{}
            binary_hashes = $binaryHashes
            runner_fingerprint_sha256 = $identity.runner_fingerprint_sha256
            platform = "windows-x86_64"
            run_id = $collectionRunId
            cohort_id = ""
            children = @($Children)
        }
        $entry.cohort_id = Get-CanonicalSha256 ([ordered]@{ scope = $entry.scope; source_sha = $entry.source_sha; subject_refs = $entry.subject_refs; platform = $entry.platform; binary_hashes = $entry.binary_hashes; runner_fingerprint_sha256 = $entry.runner_fingerprint_sha256 })
        return $entry
    }
    $entries = [System.Collections.Generic.List[object]]::new()
    foreach ($rawId in @($rawChildren | Sort-Object)) {
        $path = $null
        if ($rawId -match '^attribution-matrix-raw/([^/]+)/([^/]+)/process-(\d+)$') {
            $path = "raw/{0}-{1}-process-{2}.json" -f $Matches[1], $Matches[2], $Matches[3]
        } elseif ($rawId -match '^attribution-matrix-raw/([^/]+)/([^/]+)/unsupported$') {
            $path = "raw/{0}-{1}-unsupported.json" -f $Matches[1], $Matches[2]
        }
        if ($path) { $entries.Add((New-FragmentEntry "attribution-matrix-raw" $rawId "raw" "rssh.stage7.metric-raw/v1" $path @())) }
    }
    $entries.Add((New-FragmentEntry "attribution-matrix-aggregate" "attribution-matrix-aggregate" "aggregate" "rssh.stage7.metric-aggregate/v1" "attribution-matrix-aggregate.json" @($rawChildren | Sort-Object)))
    $fragment = [ordered]@{
        schema = "rssh.stage7-artifact-manifest-fragment/v1"
        requested_state = "attribution-ready"
        certified_commit = $sourceSha
        epoch_id = Get-CanonicalSha256 ([ordered]@{ state = "attribution-ready"; certified_commit = $sourceSha; rssh = $null; rterm = $null })
        rssh = $null
        rterm = $null
        entries = @($entries)
    }
    Write-AtomicJson -Path $fragmentPath -Value $fragment
}

$summary = [ordered]@{
    schema = "rssh.stage7/attribution-matrix-run/v1"
    ok = -not $fatalProductFailure
    certification_eligible = $certificationEligible -and -not $fatalProductFailure
    raw_process_count = $rawChildren.Count
    raw_artifact_count = $rawChildren.Count
    measured_cold_processes = $Samples
    samples_per_process = $samplesPerProcess
    artifact_manifest_fragment = if (-not $fatalProductFailure) { "artifact-manifest-fragment.json" } else { $null }
    aggregate = "attribution-matrix-aggregate.json"
    failure_classifications = @($failures)
}

if ($null -eq $previousScale) { Remove-Item Env:RSSH_BENCHMARK_WINDOW_SCALE_FACTOR -ErrorAction SilentlyContinue } else { $env:RSSH_BENCHMARK_WINDOW_SCALE_FACTOR = $previousScale }
$summary | ConvertTo-Json -Depth 30 -Compress
if ($fatalProductFailure) { exit 1 }
