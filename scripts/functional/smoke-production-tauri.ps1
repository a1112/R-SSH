[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [string] $Binary,

  [Parameter(Mandatory = $true)]
  [string] $EvidenceDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$evidence = New-Item -ItemType Directory -Force -Path $EvidenceDirectory
$stdout = Join-Path $evidence.FullName "stdout"
$stderr = Join-Path $evidence.FullName "stderr"
$processTree = Join-Path $evidence.FullName "process-tree.json"
$screenshot = Join-Path $evidence.FullName "failure-screenshot.png"
$process = $null
$ownedProcessIds = @()

function Get-Descendants([uint32] $RootProcessId) {
  $processes = @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CommandLine)
  $pending = [System.Collections.Generic.Queue[uint32]]::new()
  $pending.Enqueue($RootProcessId)
  $result = [System.Collections.Generic.List[object]]::new()
  while ($pending.Count -gt 0) {
    $parent = $pending.Dequeue()
    foreach ($candidate in $processes | Where-Object ParentProcessId -eq $parent) {
      $result.Add($candidate)
      $pending.Enqueue([uint32]$candidate.ProcessId)
    }
  }
  return @($result)
}

function Get-SessionDescendants([uint32] $RootProcessId) {
  return @(
    Get-Descendants $RootProcessId |
      Where-Object { $_.Name -ine "msedgewebview2.exe" }
  )
}

function Test-ProcessesExited([uint32[]] $ProcessIds) {
  foreach ($ownedProcessId in $ProcessIds) {
    if ($null -ne (Get-Process -Id $ownedProcessId -ErrorAction SilentlyContinue)) {
      return $false
    }
  }
  return $true
}

function Wait-Condition([scriptblock] $Condition, [int] $Seconds, [string] $Failure) {
  $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
  do {
    if (& $Condition) { return }
    [System.Threading.Thread]::Yield() | Out-Null
  } while ([DateTime]::UtcNow -lt $deadline)
  throw $Failure
}

function Save-FailureScreenshot {
  Add-Type -AssemblyName System.Drawing
  Add-Type -AssemblyName System.Windows.Forms
  $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $image = New-Object Drawing.Bitmap $bounds.Width, $bounds.Height
  $graphics = [Drawing.Graphics]::FromImage($image)
  try {
    $graphics.CopyFromScreen($bounds.Location, [Drawing.Point]::Empty, $bounds.Size)
    $image.Save($screenshot, [Drawing.Imaging.ImageFormat]::Png)
  } finally {
    $graphics.Dispose()
    $image.Dispose()
  }
}

try {
  $process = Start-Process -FilePath (Resolve-Path $Binary) -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
  Wait-Condition {
    $live = Get-Process -Id $process.Id -ErrorAction SilentlyContinue
    $null -ne $live -and $live.MainWindowHandle -ne 0
  } 30 "production Tauri window did not become visible"
  Wait-Condition { @(Get-SessionDescendants $process.Id).Count -gt 0 } 30 "production Tauri did not start a PTY child"

  & "$PSScriptRoot/windows-send-input.ps1" -ProcessId $process.Id -Action focus
  & "$PSScriptRoot/windows-send-input.ps1" -ProcessId $process.Id -Action click -ActionArgumentsJson '["80","80","left"]'
  & "$PSScriptRoot/windows-send-input.ps1" -ProcessId $process.Id -Action type -ActionArgumentsJson '["exit 7"]'
  & "$PSScriptRoot/windows-send-input.ps1" -ProcessId $process.Id -Action key -ActionArgumentsJson '["enter"]'
  Wait-Condition { @(Get-SessionDescendants $process.Id).Count -eq 0 } 15 "production Tauri PTY child did not exit after OS keyboard input"

  $ownedProcessIds = @(Get-Descendants $process.Id | ForEach-Object { [uint32]$_.ProcessId })
  & "$PSScriptRoot/windows-send-input.ps1" -ProcessId $process.Id -Action window -ActionArgumentsJson '["close"]'
  Wait-Condition { $null -eq (Get-Process -Id $process.Id -ErrorAction SilentlyContinue) } 10 "production Tauri did not exit after the verified close request"
  Wait-Condition { Test-ProcessesExited $ownedProcessIds } 10 "production Tauri left an owned WebView or helper process"
  $process.WaitForExit()
  if ($process.ExitCode -ne 0) {
    throw "production Tauri exited with code $($process.ExitCode)"
  }
  @{
    schema = 1
    root_process_id = $process.Id
    owned_process_ids = $ownedProcessIds
    remaining_owned_processes = 0
    reaped = $true
    pty_interaction = "exit 7"
  } | ConvertTo-Json | Set-Content -Encoding utf8 $processTree
} catch {
  if ($null -ne $process) {
    @{
      schema = 1
      root_process_id = $process.Id
      owned_process_ids = $ownedProcessIds
      remaining_owned_processes = @($ownedProcessIds | Where-Object { $null -ne (Get-Process -Id $_ -ErrorAction SilentlyContinue) }).Count + [int](-not $process.HasExited)
      reaped = $false
      error = $_.Exception.Message
    } | ConvertTo-Json | Set-Content -Encoding utf8 $processTree
  }
  Save-FailureScreenshot
  throw
} finally {
  if ($null -ne $process -and -not $process.HasExited) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    $process.WaitForExit()
  }
}
