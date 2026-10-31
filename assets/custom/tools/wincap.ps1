param([string]$Match = '', [string]$Out = '')
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System; using System.Text; using System.Runtime.InteropServices;
public class Win {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  public struct R { public int L, T, Ri, B; }
}
'@
$found = New-Object System.Collections.ArrayList
$cb = [Win+EnumProc]{ param($h, $l)
  if ([Win]::IsWindowVisible($h)) {
    $sb = New-Object Text.StringBuilder 512
    [Win]::GetWindowText($h, $sb, 512) | Out-Null
    $t = $sb.ToString()
    if ($t) {
      $procId = 0; [Win]::GetWindowThreadProcessId($h, [ref]$procId) | Out-Null
      $name = (Get-Process -Id $procId -ErrorAction SilentlyContinue).ProcessName
      [void]$found.Add([pscustomobject]@{ Handle = $h; Title = $t; Proc = $name })
    }
  }
  $true }
[Win]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
if (-not $Match) {
  $found | Where-Object { $_.Proc -match 'Terminal|conhost|OpenConsole|cmd|cracked' } | Format-Table Handle, Proc, Title -AutoSize | Out-String -Width 220
  return
}
$w = $found | Where-Object { $_.Title -match $Match } | Select-Object -First 1
if (-not $w) { "no window matching '$Match'"; return }
[Win]::ShowWindow($w.Handle, 9) | Out-Null
[Win]::SetForegroundWindow($w.Handle) | Out-Null
Start-Sleep -Milliseconds 800
$r = New-Object Win+R
[Win]::GetWindowRect($w.Handle, [ref]$r) | Out-Null
$bw = $r.Ri - $r.L; $bh = $r.B - $r.T
$bmp = New-Object Drawing.Bitmap $bw, $bh
$g = [Drawing.Graphics]::FromImage($bmp)
$dc = $g.GetHdc()
[Win]::PrintWindow($w.Handle, $dc, 2) | Out-Null
$g.ReleaseHdc($dc)
$bmp.Save($Out)
"captured '$($w.Title)' ($($w.Proc)) ${bw}x${bh} -> $Out"
