[CmdletBinding()]
param(
  [uint32] $ProcessId = 0,

  [string] $WindowTitle,

  [UInt64] $WindowHandle = 0,

  [Parameter(Mandatory = $true)]
  [ValidateSet("focus", "type", "key", "click", "drag", "wheel", "paste", "resize", "window")]
  [string] $Action,

  [string] $ActionArgumentsJson = "[]"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ActionArguments = [string[]](ConvertFrom-Json -InputObject $ActionArgumentsJson)

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class RsshFunctionalInput {
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr lParam);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public InputUnion u; }
    [StructLayout(LayoutKind.Explicit)] public struct InputUnion {
        [FieldOffset(0)] public MOUSEINPUT mi;
        [FieldOffset(0)] public KEYBDINPUT ki;
    }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT {
        public int dx; public int dy; public uint mouseData; public uint dwFlags;
        public uint time; public UIntPtr dwExtraInfo;
    }
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT {
        public ushort wVk; public ushort wScan; public uint dwFlags;
        public uint time; public UIntPtr dwExtraInfo;
    }

    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr value);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int length);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hwnd, ref POINT point);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll", SetLastError=true)] public static extern uint SendInput(uint count, INPUT[] inputs, int size);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr hwnd, int x, int y, int width, int height, bool repaint);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint message, UIntPtr wParam, IntPtr lParam);

    const uint INPUT_MOUSE = 0;
    const uint INPUT_KEYBOARD = 1;
    const uint KEYEVENTF_KEYUP = 0x0002;
    const uint KEYEVENTF_UNICODE = 0x0004;

    public static IntPtr FindWindow(uint expectedProcessId) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((hwnd, _) => {
            uint actual;
            GetWindowThreadProcessId(hwnd, out actual);
            if (actual == expectedProcessId && IsWindowVisible(hwnd)) { found = hwnd; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static IntPtr FindWindowByTitle(string expectedTitle) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((hwnd, _) => {
            if (!IsWindowVisible(hwnd)) return true;
            StringBuilder title = new StringBuilder(512);
            GetWindowText(hwnd, title, title.Capacity);
            if (title.ToString().IndexOf(expectedTitle, StringComparison.OrdinalIgnoreCase) >= 0) { found = hwnd; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static void UnicodeText(string text) {
        foreach (char codeUnit in text) {
            INPUT down = new INPUT { type = INPUT_KEYBOARD };
            down.u.ki.wScan = codeUnit;
            down.u.ki.dwFlags = KEYEVENTF_UNICODE;
            INPUT up = down;
            up.u.ki.dwFlags = KEYEVENTF_UNICODE | KEYEVENTF_KEYUP;
            SendChecked(new [] { down, up });
        }
    }

    public static void VirtualKey(ushort key, bool down) {
        INPUT input = new INPUT { type = INPUT_KEYBOARD };
        input.u.ki.wVk = key;
        input.u.ki.dwFlags = down ? 0u : KEYEVENTF_KEYUP;
        SendChecked(new [] { input });
    }

    public static void Mouse(uint flags, uint data) {
        INPUT input = new INPUT { type = INPUT_MOUSE };
        input.u.mi.dwFlags = flags;
        input.u.mi.mouseData = data;
        SendChecked(new [] { input });
    }

    static void SendChecked(INPUT[] inputs) {
        if (SendInput((uint)inputs.Length, inputs, Marshal.SizeOf(typeof(INPUT))) != inputs.Length) {
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "SendInput failed");
        }
    }
}
'@

$window = if ($WindowHandle -ne 0) {
  [IntPtr]::new([Int64]$WindowHandle)
} elseif ($WindowTitle) {
  $deadline = [DateTime]::UtcNow.AddSeconds(10)
  do {
    $candidate = [RsshFunctionalInput]::FindWindowByTitle($WindowTitle)
    if ($candidate -ne [IntPtr]::Zero) { break }
    Start-Sleep -Milliseconds 10
  } while ([DateTime]::UtcNow -lt $deadline)
  $candidate
} else {
  [RsshFunctionalInput]::FindWindow($ProcessId)
}
if ($window -eq [IntPtr]::Zero) {
  $titles = Get-Process | Where-Object { $_.MainWindowTitle } | ForEach-Object { "$($_.Id):$($_.ProcessName):$($_.MainWindowTitle)" }
  throw "no visible top-level window matches process=$ProcessId title=$WindowTitle; visible=$($titles -join '|')"
}
if (-not [RsshFunctionalInput]::SetForegroundWindow($window)) {
  throw "SetForegroundWindow failed for process=$ProcessId title=$WindowTitle"
}

function Set-ClientCursor([int] $X, [int] $Y) {
  $point = [RsshFunctionalInput+POINT]::new()
  $point.X = $X
  $point.Y = $Y
  if (-not [RsshFunctionalInput]::ClientToScreen($window, [ref]$point)) {
    throw "ClientToScreen failed"
  }
  if (-not [RsshFunctionalInput]::SetCursorPos($point.X, $point.Y)) {
    throw "SetCursorPos failed"
  }
}

function Convert-WheelData([int] $Delta) {
  return [BitConverter]::ToUInt32([BitConverter]::GetBytes($Delta), 0)
}

function Send-VirtualKey([uint16] $Key) {
  [RsshFunctionalInput]::VirtualKey($Key, $true)
  [RsshFunctionalInput]::VirtualKey($Key, $false)
}

switch ($Action) {
  "focus" {}
  "type" { [RsshFunctionalInput]::UnicodeText(($ActionArguments -join " ")) }
  "key" {
    $keys = @{ enter = 0x0D; tab = 0x09; escape = 0x1B; backspace = 0x08; up = 0x26; down = 0x28; left = 0x25; right = 0x27 }
    $name = $ActionArguments[0].ToLowerInvariant()
    if (-not $keys.ContainsKey($name)) { throw "unsupported key $name" }
    Send-VirtualKey $keys[$name]
  }
  "click" {
    Set-ClientCursor ([int]$ActionArguments[0]) ([int]$ActionArguments[1])
    $buttons = @{ left = @(0x0002, 0x0004); right = @(0x0008, 0x0010); middle = @(0x0020, 0x0040) }
    $flags = $buttons[$ActionArguments[2]]
    if ($null -eq $flags) { throw "unsupported mouse button" }
    [RsshFunctionalInput]::Mouse($flags[0], 0); [RsshFunctionalInput]::Mouse($flags[1], 0)
  }
  "drag" {
    $buttons = @{ left = @(0x0002, 0x0004); right = @(0x0008, 0x0010); middle = @(0x0020, 0x0040) }
    $flags = $buttons[$ActionArguments[4]]
    Set-ClientCursor ([int]$ActionArguments[0]) ([int]$ActionArguments[1])
    [RsshFunctionalInput]::Mouse($flags[0], 0)
    Set-ClientCursor ([int]$ActionArguments[2]) ([int]$ActionArguments[3])
    [RsshFunctionalInput]::Mouse($flags[1], 0)
  }
  "wheel" {
    [RsshFunctionalInput]::Mouse(0x0800, (Convert-WheelData ([int]$ActionArguments[1] * 120)))
    [RsshFunctionalInput]::Mouse(0x01000, (Convert-WheelData ([int]$ActionArguments[0] * 120)))
  }
  "paste" {
    Set-Clipboard -Value ($ActionArguments -join " ")
    [RsshFunctionalInput]::VirtualKey(0x10, $true)
    [RsshFunctionalInput]::VirtualKey(0x11, $true)
    Send-VirtualKey 0x56
    [RsshFunctionalInput]::VirtualKey(0x11, $false)
    [RsshFunctionalInput]::VirtualKey(0x10, $false)
  }
  "resize" {
    $rect = [RsshFunctionalInput+RECT]::new()
    if (-not [RsshFunctionalInput]::GetWindowRect($window, [ref]$rect)) { throw "GetWindowRect failed" }
    if (-not [RsshFunctionalInput]::MoveWindow($window, $rect.Left, $rect.Top, [int]$ActionArguments[0], [int]$ActionArguments[1], $true)) { throw "MoveWindow failed" }
  }
  "window" {
    switch ($ActionArguments[0]) {
      "minimize" { [void][RsshFunctionalInput]::ShowWindow($window, 6) }
      "maximize" { [void][RsshFunctionalInput]::ShowWindow($window, 3) }
      "restore" { [void][RsshFunctionalInput]::ShowWindow($window, 9) }
      "close" {
        if (-not [RsshFunctionalInput]::PostMessage($window, 0x0010, [UIntPtr]::Zero, [IntPtr]::Zero)) {
          throw "PostMessage(WM_CLOSE) failed"
        }
      }
      default { throw "unsupported window operation $($ActionArguments[0])" }
    }
  }
}
