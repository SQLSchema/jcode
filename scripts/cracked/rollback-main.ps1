[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'Medium')]
param(
    [switch]$Force,
    [ValidateRange(1, 3600)][int]$WaitSeconds = 60
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Import definitions only. Pass through shared parameters so dot-sourcing does not reset them.
. (Join-Path $PSScriptRoot 'apply-to-main.ps1') -Force:$Force -WaitSeconds $WaitSeconds -WhatIf:$WhatIfPreference
$paths = Get-JcodeMainPaths
Assert-JcodeMainStopped $paths -WaitForClose:$Force -Seconds $WaitSeconds -DryRun:$WhatIfPreference
# Copy preserves mtimes, so select by the sortable UTC transaction name, NOT mtime.
$backups = @(Get-ChildItem -LiteralPath (Split-Path $paths.Exe) -Filter 'jcode.exe.stock-*' -File |
    Where-Object { $_.Name -match '^jcode\.exe\.stock-\d{8}T\d{13}Z$' } | Sort-Object Name -Descending)
if (-not $backups.Count) { throw 'No main-install executable backup found. Nothing restored.' }
$exeBackup = $backups[0].FullName
$stamp = $backups[0].Name.Substring('jcode.exe.stock-'.Length)
$configBackup = $paths.Config + '.bak-' + $stamp
foreach ($path in @($exeBackup, $configBackup)) {
    Assert-JcodePlainPath $path
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Newest backup pair is incomplete: $path. Refusing to mix transactions." }
}
Write-Host "Restore executable: $exeBackup -> $($paths.Exe)"
Write-Host "Restore config ONLY: $configBackup -> $($paths.Config)"
Write-Host 'Newest matched backup pair retained. No auth, sessions, environment, channel, or PATH changes.'
Write-Host 'Rollback restores the previous update preference, including true if that was backed up.'
if (-not $PSCmdlet.ShouldProcess($paths.Exe, 'Restore newest matched executable and config backups, byte-for-byte')) { return }
$exeHash = Get-JcodeFileHash $exeBackup
$configHash = Get-JcodeFileHash $configBackup
$currentExeHash = Get-JcodeFileHash $paths.Exe
$currentConfigHash = Get-JcodeFileHash $paths.Config
$token = [Guid]::NewGuid().ToString('N')
$stage = Join-Path (Split-Path $paths.Exe) ("jcode-restore-$token.exe")
$recoveryExe = Join-Path (Split-Path $paths.Exe) ("jcode-recovery-$token.exe")
$recoveryConfig = $paths.Config + ".recovery-$token"
$changed = $false
$completed = $false
$recovered = $false
try {
    [IO.File]::Copy($exeBackup, $stage, $false)
    [IO.File]::Copy($paths.Config, $recoveryConfig, $false)
    if ((Get-JcodeFileHash $stage) -ne $exeHash -or (Get-JcodeFileHash $recoveryConfig) -ne $currentConfigHash) { throw 'Rollback staging integrity check failed.' }
    Assert-JcodeMainStopped $paths
    if ((Get-JcodeFileHash $paths.Exe) -ne $currentExeHash -or (Get-JcodeFileHash $paths.Config) -ne $currentConfigHash) { throw 'Main files changed during rollback preflight. Retry.' }
    [IO.File]::Replace($stage, $paths.Exe, $recoveryExe)
    $changed = $true
    [IO.File]::Copy($configBackup, $paths.Config, $true)
    if ((Get-JcodeFileHash $paths.Exe) -ne $exeHash -or (Get-JcodeFileHash $paths.Config) -ne $configHash) { throw 'Restored file integrity check failed.' }
    $completed = $true
    Write-Host 'Rollback verified: executable and config match their backups byte-for-byte.'
} catch {
    $failure = $_
    if ($changed) {
        [IO.File]::Copy($recoveryExe, $paths.Exe, $true)
        [IO.File]::Copy($recoveryConfig, $paths.Config, $true)
        $recovered = $true
        Write-Host 'Rollback failed. Restored the pre-rollback executable and config.'
    }
    throw $failure
} finally {
    # If recovery itself throws, keep recovery files for manual repair.
    if (-not $changed -or $completed -or $recovered) {
        foreach ($path in @($stage, $recoveryExe, $recoveryConfig)) { if (Test-Path -LiteralPath $path) { [IO.File]::Delete($path) } }
    }
}
