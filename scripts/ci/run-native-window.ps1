[CmdletBinding()]
param(
  [switch] $HarnessSelfTest,

  [ValidateSet("debug", "release")]
  [string] $Profile = "debug",

  [string] $ExpectedTarget,

  [ValidateSet("windows-conpty", "unix-pty")]
  [string] $ExpectedPtyBackend
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

. (Join-Path $PSScriptRoot "process-harness.ps1")

$profileArguments = if ($Profile -eq "release") { @("--release") } else { @() }
$profileDirectory = if ($Profile -eq "release") { "release" } else { "debug" }
$targetDirectory = if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
  Join-Path $repositoryRoot "target"
} else {
  $env:CARGO_TARGET_DIR
}
$executable = Join-Path $targetDirectory "$profileDirectory/rssh-app.exe"

Assert-BoundedProcessHarness

if ($HarnessSelfTest) {
  return
}
if ([string]::IsNullOrWhiteSpace($ExpectedTarget) -or [string]::IsNullOrWhiteSpace($ExpectedPtyBackend)) {
  throw "-ExpectedTarget and -ExpectedPtyBackend are required outside -HarnessSelfTest"
}

Push-Location $repositoryRoot
try {
  $buildArguments = @("build", "--locked", "-p", "rssh-app", "--all-targets") + $profileArguments
  $null = Invoke-BoundedProcess -Phase "native E2E build ($Profile)" -FilePath "cargo" -ArgumentList $buildArguments -TimeoutSeconds 1200

  $versionResult = Invoke-BoundedProcess -Phase "version identity" -FilePath $executable -ArgumentList @("version", "--json") -TimeoutSeconds 30
  $version = $versionResult.Stdout | ConvertFrom-Json
  if ($version.target -ne $ExpectedTarget) {
    throw "version target mismatch: observed '$($version.target)', expected '$ExpectedTarget'"
  }
  if ($version.pty_backend -ne $ExpectedPtyBackend) {
    throw "PTY backend mismatch: observed '$($version.pty_backend)', expected '$ExpectedPtyBackend'"
  }

  $null = Invoke-BoundedProcess -Phase "OpenSSH client probe" -FilePath "ssh" -ArgumentList @("-V") -TimeoutSeconds 15

  $nativeSshArguments = @(
    "test", "--locked", "-p", "rssh-ssh", "--all-targets"
  ) + $profileArguments + @("--", "--nocapture")
  $null = Invoke-BoundedProcess -Phase "hermetic native SSH tests ($Profile)" -FilePath "cargo" -ArgumentList $nativeSshArguments -TimeoutSeconds 300

  $env:RSSH_REQUIRE_OPENSSH = "1"
  $openSshArguments = @(
    "test", "--locked", "-p", "rssh-app", "--test", "openssh_loopback"
  ) + $profileArguments + @("--", "--nocapture")
  $null = Invoke-BoundedProcess -Phase "system OpenSSH interoperability ($Profile)" -FilePath "cargo" -ArgumentList $openSshArguments -TimeoutSeconds 300

  $testArguments = @(
    "test", "--locked", "-p", "rssh-app", "--all-targets"
  ) + $profileArguments + @(
    "native_window_e2e_presents_ten_frames_from_a_real_pty",
    "--", "--exact", "--nocapture"
  )
  $null = Invoke-BoundedProcess -Phase "native ten-frame E2E ($Profile)" -FilePath "cargo" -ArgumentList $testArguments -TimeoutSeconds 180

  foreach ($scenario in @(
    "native_window_e2e_preserves_gpu_text_at_scale_100",
    "native_window_e2e_preserves_gpu_text_at_scale_125",
    "native_window_e2e_preserves_gpu_text_at_scale_150",
    "native_window_e2e_preserves_gpu_text_at_scale_200",
    "native_window_local_pane_v2_writes_visible_session_log"
  )) {
    $scenarioArguments = @(
      "test", "--locked", "-p", "rssh-app", "--test", "native_window_e2e"
    ) + $profileArguments + @(
      $scenario,
      "--", "--exact", "--ignored", "--nocapture"
    )
    $null = Invoke-BoundedProcess -Phase "native E2E scenario $scenario ($Profile)" -FilePath "cargo" -ArgumentList $scenarioArguments -TimeoutSeconds 300
  }
} finally {
  Pop-Location
}
