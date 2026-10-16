<#
.SYNOPSIS
  Captures a top-level window to a PNG for native visual verification.

.DESCRIPTION
  Grabs a window owned by the given process. It first tries
  PrintWindow(PW_RENDERFULLCONTENT), which is what usually captures a WebView2
  surface without the desktop compositor; if the result is a flat image (the
  common failure mode for DirectComposition content) it falls back to a screen
  BitBlt of the window rectangle, which requires the window to be visible and
  unobstructed.

.PARAMETER ProcessId
  Process that owns the window.

.PARAMETER OutputPath
  Destination PNG. Overwritten.

.PARAMETER Width
  Optional width to resize the window to before capturing (height follows the
  current window height).

.PARAMETER FromScreen
  Skip PrintWindow and capture the screen rectangle directly.

.EXAMPLE
  powershell -File scripts/capture-window.ps1 -ProcessId 1234 -OutputPath shot.png -Width 520
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)] [int]$ProcessId,
  [Parameter(Mandatory)] [string]$OutputPath,
  [int]$Width = 0,
  [switch]$FromScreen
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

Add-Type -Namespace WindowCapture -Name Native -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
[DllImport("user32.dll", SetLastError = true)] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
[DllImport("user32.dll", SetLastError = true)] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdc, uint flags);
[DllImport("user32.dll", SetLastError = true)] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
[DllImport("user32.dll", SetLastError = true)] public static extern bool SetForegroundWindow(IntPtr hWnd);
[DllImport("user32.dll", SetLastError = true)] public static extern bool ShowWindow(IntPtr hWnd, int command);
'@ | Out-Null

$SWP_NOZORDER = 0x0004
$SWP_NOACTIVATE = 0x0010
$SW_SHOW = 5

function Get-WindowHandle {
  param([int]$Id)
  $process = Get-Process -Id $Id -ErrorAction Stop
  for ($attempt = 0; $attempt -lt 40; $attempt++) {
    $process.Refresh()
    if ($process.MainWindowHandle -ne [IntPtr]::Zero) { return $process.MainWindowHandle }
    Start-Sleep -Milliseconds 250
  }
  throw "process $Id has no main window handle"
}

function Save-WindowBitmap {
  param([IntPtr]$Handle, [string]$Path, [bool]$UsePrintWindow)
  $rect = New-Object WindowCapture.Native+RECT
  if (-not [WindowCapture.Native]::GetWindowRect($Handle, [ref]$rect)) {
    throw 'GetWindowRect failed'
  }
  $width = $rect.Right - $rect.Left
  $height = $rect.Bottom - $rect.Top
  if ($width -le 0 -or $height -le 0) { throw "window has invalid bounds ${width}x${height}" }

  $bitmap = New-Object System.Drawing.Bitmap($width, $height)
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  try {
    if ($UsePrintWindow) {
      $hdc = $graphics.GetHdc()
      try {
        [void][WindowCapture.Native]::PrintWindow($Handle, $hdc, 2)
      } finally {
        $graphics.ReleaseHdc($hdc)
      }
    } else {
      $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($width, $height)))
    }
  } finally {
    $graphics.Dispose()
  }
  $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
  $bitmap.Dispose()
}

# A flat PrintWindow result (one colour dominating) means the WebView surface
# was not rendered; fall back to the screen rectangle.
function Test-BitmapHasContent {
  param([string]$Path)
  $bitmap = New-Object System.Drawing.Bitmap($Path)
  try {
    $samples = New-Object 'System.Collections.Generic.HashSet[int]'
    for ($y = 0; $y -lt $bitmap.Height; $y += [Math]::Max(1, [int]($bitmap.Height / 16))) {
      for ($x = 0; $x -lt $bitmap.Width; $x += [Math]::Max(1, [int]($bitmap.Width / 16))) {
        [void]$samples.Add($bitmap.GetPixel($x, $y).ToArgb())
      }
    }
    return $samples.Count -gt 3
  } finally {
    $bitmap.Dispose()
  }
}

$handle = Get-WindowHandle -Id $ProcessId
[void][WindowCapture.Native]::ShowWindow($handle, $SW_SHOW)
if ($Width -gt 0) {
  $rect = New-Object WindowCapture.Native+RECT
  [void][WindowCapture.Native]::GetWindowRect($handle, [ref]$rect)
  $height = $rect.Bottom - $rect.Top
  [void][WindowCapture.Native]::SetWindowPos($handle, [IntPtr]::Zero, $rect.Left, $rect.Top, $Width, $height, $SWP_NOZORDER -bor $SWP_NOACTIVATE)
  Start-Sleep -Milliseconds 600
}

[void][WindowCapture.Native]::SetForegroundWindow($handle)
Start-Sleep -Milliseconds 400

if ($FromScreen) {
  Save-WindowBitmap -Handle $handle -Path $OutputPath -UsePrintWindow $false
} else {
  Save-WindowBitmap -Handle $handle -Path $OutputPath -UsePrintWindow $true
  if (-not (Test-BitmapHasContent -Path $OutputPath)) {
    Write-Host 'PrintWindow produced a flat image; falling back to the screen rectangle.'
    Save-WindowBitmap -Handle $handle -Path $OutputPath -UsePrintWindow $false
  }
}

Write-Host "Captured $OutputPath"
