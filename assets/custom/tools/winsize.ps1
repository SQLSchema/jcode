param([string]$Match = 'cracked[/ ]', [int]$Width = 1920, [int]$Height = 1040)
Add-Type @'
using System; using System.Text; using System.Runtime.InteropServices;
public class WinSize {
  public delegate bool E(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(E f, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool r);
}
'@
$script:done = $false
[WinSize]::EnumWindows({ param($h, $l)
  if ([WinSize]::IsWindowVisible($h)) {
    $s = New-Object Text.StringBuilder 256
    [void][WinSize]::GetWindowText($h, $s, 256)
    if ($s.ToString() -match $Match) {
      $ok = [WinSize]::MoveWindow($h, 0, 0, $Width, $Height, $true)
      "moved=$ok '$($s.ToString())' -> ${Width}x${Height}"
      $script:done = $true
      return $false
    }
  }
  $true }, [IntPtr]::Zero) | Out-Null
if (-not $script:done) { "no window matching '$Match'" }
