# Main install only. Dot-sourcing exposes shared safety helpers without executing a swap.
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'Medium')]
param(
    [switch]$Theme,
    [switch]$Force,
    [ValidateRange(1, 3600)][int]$WaitSeconds = 60,
    # Optional prebuilt executable, also used by the auth-free sandbox test.
    [string]$ArtifactPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-JcodePlainPath([string]$Path) {
    $cursor = [IO.Path]::GetFullPath($Path)
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            if ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw "Refusing linked/reparse path: $cursor"
            }
        }
        $parent = [IO.Path]::GetDirectoryName($cursor)
        if ($parent -eq $cursor) { break }
        $cursor = $parent
    }
}

function Get-JcodeMainPaths {
    if (-not $env:LOCALAPPDATA -or -not $env:USERPROFILE) { throw 'LOCALAPPDATA and USERPROFILE must be set.' }
    # Even JCODE_HOME=.jcode changes external credential resolution (storage/lib.rs).
    if (Test-Path Env:JCODE_HOME) { throw 'Unset JCODE_HOME first. It redirects external Claude/Codex credential paths, even when set to .jcode.' }
    $root = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'jcode'))
    $exe = Join-Path $root 'bin\jcode.exe'
    $config = Join-Path $env:USERPROFILE '.jcode\config.toml'
    if ($env:JCODE_INSTALL_DIR -and [IO.Path]::GetFullPath($env:JCODE_INSTALL_DIR).TrimEnd('\') -ine (Split-Path $exe).TrimEnd('\')) {
        throw 'Unset the non-main JCODE_INSTALL_DIR before using these scripts.'
    }
    foreach ($path in @($exe, $config)) {
        Assert-JcodePlainPath $path
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Required existing file missing: $path" }
    }
    [pscustomobject]@{ Root = $root; Exe = $exe; Config = $config }
}

function Get-JcodeMainProcesses($Paths) {
    # Read-only query. Do not send -WhatIf into the CIM module's alias setup.
    $WhatIfPreference = $false
    # Include stock shared-server/current builds, not the isolated jcode-cracked package.
    # Unknown executable paths are blockers: access denied is not evidence of safety.
    @(Get-CimInstance Win32_Process -Filter "Name = 'jcode.exe' OR Name = 'cracked.exe'" -ErrorAction Stop |
        Where-Object { -not $_.ExecutablePath -or $_.ExecutablePath.StartsWith($Paths.Root.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase) } |
        Select-Object ProcessId, Name, ExecutablePath)
}

function Assert-JcodeMainStopped($Paths, [switch]$WaitForClose, [int]$Seconds = 60, [switch]$DryRun) {
    $running = @(Get-JcodeMainProcesses $Paths)
    if (-not $running.Count) { return }
    Write-Host 'Main-install processes still running (no process will be killed):'
    foreach ($item in $running) { Write-Host ("PID {0}  {1}  {2}" -f $item.ProcessId, $item.Name, $item.ExecutablePath) }
    if ($DryRun) {
        Write-Host 'Execution would refuse. Close ALL listed clients/servers, then retry. -Force only waits for you to close them.'
        return
    }
    if (-not $WaitForClose) { throw 'Close the listed clients/servers and retry, or use -Force to wait for manual closure. Nothing was killed.' }
    Write-Host "Waiting up to $Seconds seconds for manual closure. -Force never bypasses this check."
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        Start-Sleep -Milliseconds 250
        $running = @(Get-JcodeMainProcesses $Paths)
        if (-not $running.Count) { return }
    } while ([DateTime]::UtcNow -lt $deadline)
    throw 'Main-install processes are still running. No files changed, and nothing was killed.'
}

function Initialize-JcodeFileIO {
    if ('JcodeMainFileIO' -as [type]) { return }
    # Windows normally updates access timestamps even for read-only operations.
    # SetFileTime's -1 sentinel suppresses that update for this handle only.
    Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using Microsoft.Win32.SafeHandles;
public static class JcodeMainFileIO {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern SafeFileHandle CreateFile(string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetFileTime(SafeFileHandle file, IntPtr creation, ref long access, IntPtr write);
    private static FileStream Open(string path) {
        SafeFileHandle handle = CreateFile(path, 0x80000100, 7, IntPtr.Zero, 3, 0x80, IntPtr.Zero);
        if (handle.IsInvalid) { int error = Marshal.GetLastWin32Error(); handle.Dispose(); throw new Win32Exception(error); }
        long suppress = -1;
        if (!SetFileTime(handle, IntPtr.Zero, ref suppress, IntPtr.Zero)) {
            int error = Marshal.GetLastWin32Error(); handle.Dispose(); throw new Win32Exception(error);
        }
        return new FileStream(handle, FileAccess.Read);
    }
    public static byte[] Read(string path) {
        using (FileStream stream = Open(path))
        using (MemoryStream output = new MemoryStream()) { stream.CopyTo(output); return output.ToArray(); }
    }
    public static string Hash(string path) {
        using (FileStream stream = Open(path))
        using (SHA256 sha = SHA256.Create()) { return BitConverter.ToString(sha.ComputeHash(stream)).Replace("-", ""); }
    }
    public static string HashBytes(byte[] bytes) {
        using (SHA256 sha = SHA256.Create()) { return BitConverter.ToString(sha.ComputeHash(bytes)).Replace("-", ""); }
    }
}
'@ | Out-Null
}

function Read-JcodeConfig([string]$Path) {
    Initialize-JcodeFileIO
    $bytes = [JcodeMainFileIO]::Read($Path)
    $offset = 0
    $encoding = New-Object Text.UTF8Encoding($false, $true)
    if ($bytes.Length -ge 4 -and (($bytes[0] -eq 255 -and $bytes[1] -eq 254 -and $bytes[2] -eq 0 -and $bytes[3] -eq 0) -or ($bytes[0] -eq 0 -and $bytes[1] -eq 0 -and $bytes[2] -eq 254 -and $bytes[3] -eq 255))) {
        throw 'UTF-32 config is unsupported. Nothing changed.'
    } elseif ($bytes.Length -ge 3 -and $bytes[0] -eq 239 -and $bytes[1] -eq 187 -and $bytes[2] -eq 191) {
        $offset = 3
    } elseif ($bytes.Length -ge 2 -and $bytes[0] -eq 255 -and $bytes[1] -eq 254) {
        $encoding = New-Object Text.UnicodeEncoding($false, $false, $true)
        $offset = 2
    } elseif ($bytes.Length -ge 2 -and $bytes[0] -eq 254 -and $bytes[1] -eq 255) {
        $encoding = New-Object Text.UnicodeEncoding($true, $false, $true)
        $offset = 2
    }
    [pscustomobject]@{ Bytes = $bytes; Encoding = $encoding; Offset = $offset; Text = $encoding.GetString($bytes, $offset, $bytes.Length - $offset) }
}

function ConvertTo-JcodeConfigBytes($Original, [string]$Text) {
    $payload = $Original.Encoding.GetBytes($Text)
    $result = New-Object byte[] ($Original.Offset + $payload.Length)
    if ($Original.Offset) { [Array]::Copy($Original.Bytes, 0, $result, 0, $Original.Offset) }
    [Array]::Copy($payload, 0, $result, $Original.Offset, $payload.Length)
    return ,$result
}

function Get-JcodeTomlMask([string]$Text) {
    # Mask strings/comments, retaining indices and newlines. This prevents table/key
    # lookalikes inside multiline prompts or comments from being edited.
    $mask = $Text.ToCharArray()
    $i = 0
    while ($i -lt $Text.Length) {
        $ch = $Text[$i]
        if ($ch -eq '#') {
            while ($i -lt $Text.Length -and $Text[$i] -notin @("`r", "`n")) { $mask[$i] = ' '; $i++ }
        } elseif ($ch -eq '"' -or $ch -eq "'") {
            $quote = [string]$ch
            $triple = $i + 2 -lt $Text.Length -and $Text.Substring($i, 3) -eq ($quote * 3)
            $width = 1
            if ($triple) { $width = 3 }
            for ($n = 0; $n -lt $width; $n++) { $mask[$i] = ' '; $i++ }
            $closed = $false
            while ($i -lt $Text.Length) {
                if ($ch -eq '"' -and $Text[$i] -eq '\') {
                    $mask[$i] = ' '; $i++
                    if ($i -lt $Text.Length) { if ($Text[$i] -notin @("`r", "`n")) { $mask[$i] = ' ' }; $i++ }
                    continue
                }
                if ($i + $width -le $Text.Length -and $Text.Substring($i, $width) -eq ($quote * $width)) {
                    # TOML allows up to two extra quotes at a multiline ending.
                    for ($n = 0; $n -lt $width; $n++) { $mask[$i] = ' '; $i++ }
                    if ($triple) { while ($i -lt $Text.Length -and [string]$Text[$i] -eq $quote) { $mask[$i] = ' '; $i++ } }
                    $closed = $true
                    break
                }
                if (-not $triple -and $Text[$i] -in @("`r", "`n")) { throw 'Unsupported or malformed config string. Nothing changed.' }
                if ($Text[$i] -notin @("`r", "`n")) { $mask[$i] = ' ' }
                $i++
            }
            if (-not $closed) { throw 'Unterminated config string. Nothing changed.' }
        } else { $i++ }
    }
    -join $mask
}

function Get-JcodeTomlTablePattern([string]$Table) {
    '^\s*\[\s*' + (($Table.Split('.') | ForEach-Object { '(?:' + [regex]::Escape($_) + '|"' + [regex]::Escape($_) + '"|\x27' + [regex]::Escape($_) + '\x27)' }) -join '\s*\.\s*') + '\s*\]\s*(?:#.*)?$'
}

function Set-JcodeTomlKey([string]$Text, [string]$Table, [string]$Key, [string]$Value, [switch]$Boolean) {
    $mask = Get-JcodeTomlMask $Text
    $tablePattern = Get-JcodeTomlTablePattern $Table
    $keyPattern = '(?:' + [regex]::Escape($Key) + '|"' + [regex]::Escape($Key) + '"|\x27' + [regex]::Escape($Key) + '\x27)'
    $headers = @()
    $entries = @()
    $active = $false
    $insertAt = $Text.Length
    $depth = 0
    $currentHeader = ''
    $segments = $Table.Split('.')
    foreach ($line in [regex]::Matches($Text, '[^\r\n]*(?:\r\n|\n|\r|$)')) {
        if (-not $line.Length) { continue }
        $raw = $line.Value.TrimEnd("`r", "`n")
        $code = $mask.Substring($line.Index, $raw.Length)
        if ($depth -eq 0 -and $code -match '^\s*\[') {
            # Escaped table names could conceal a target table. Do not guess.
            if ($raw -match '^\s*\[[^\]]*\\[^\]]*\]') { throw 'Escaped table names are unsupported. Nothing changed.' }
            if ($raw -match '^\s*\[\[' -and $raw.Replace('[[', '[').Replace(']]', ']') -match $tablePattern) { throw "Target [$Table] cannot be an array of tables." }
            if ($active) { $insertAt = $line.Index }
            $currentHeader = $raw
            $active = $raw -match $tablePattern
            if ($active) { $headers += $line; $insertAt = $Text.Length }
            continue
        }
        if ($depth -eq 0 -and $code -match '=') {
            if ($raw -match '^\s*"[^"]*\\[^"]*"\s*(?:=|\.)') { throw 'Escaped config key names are unsupported. Nothing changed.' }
            # Reject target tables already defined through inline/dotted keys,
            # including colors = {...} under [display] and quoted root keys.
            for ($part = 0; $part -lt $segments.Length; $part++) {
                $inParent = -not $currentHeader -and $part -eq 0
                if ($part -gt 0) { $inParent = $currentHeader -match (Get-JcodeTomlTablePattern (($segments[0..($part - 1)]) -join '.')) }
                $name = [regex]::Escape($segments[$part])
                if ($inParent -and $raw -match ('^\s*(?:' + $name + '|"' + $name + '"|\x27' + $name + '\x27)\s*[=.]')) { throw "Inline/dotted $Table configuration is unsupported. Nothing changed." }
            }
            if ($active -and $raw -match ('^\s*' + $keyPattern + '\s*\.')) { throw "Dotted target key [$Table] $Key is unsupported." }
            if ($active -and $raw -match ('^\s*' + $keyPattern + '\s*=')) { $entries += $line }
        }
        foreach ($token in $code.ToCharArray()) {
            if ($token -in @('[', '{')) { $depth++ }
            if ($token -in @(']', '}')) { $depth-- }
            if ($depth -lt 0) { throw 'Unbalanced config array/inline table. Nothing changed.' }
        }
    }
    if ($depth -ne 0) { throw 'Unbalanced config array/inline table. Nothing changed.' }
    if ($headers.Count -gt 1 -or $entries.Count -gt 1) { throw "Duplicate target table/key: [$Table] $Key. Nothing changed." }
    if ($entries.Count) {
        $line = $entries[0]
        $valuePattern = '(?:"#[0-9a-fA-F]{6}"|\x27#[0-9a-fA-F]{6}\x27)'
        if ($Boolean) { $valuePattern = '(?:true|false)' }
        $match = [regex]::Match($line.Value.TrimEnd("`r", "`n"), '^(?<prefix>\s*' + $keyPattern + '\s*=\s*)(?<value>' + $valuePattern + ')(?<tail>\s*(?:#.*)?)$')
        if (-not $match.Success) { throw "Unsupported value for [$Table] $Key. Nothing changed." }
        $at = $line.Index + $match.Groups['value'].Index
        return $Text.Substring(0, $at) + $Value + $Text.Substring($at + $match.Groups['value'].Length)
    }
    $newline = "`r`n"
    $firstNewline = [regex]::Match($Text, '\r\n|\n|\r')
    if ($firstNewline.Success) { $newline = $firstNewline.Value }
    if (-not $headers.Count) {
        # Do not introduce a table already defined through inline/dotted syntax.
        $rootKey = [regex]::Escape($Table.Split('.')[0])
        if ($mask -match ('(?m)^\s*' + $rootKey + '\s*[=.]')) { throw "Inline/dotted $rootKey configuration is unsupported. Nothing changed." }
        $prefix = ''
        if ($Text.Length -and $Text[-1] -notin @("`r", "`n")) { $prefix = $newline }
        return $Text + $prefix + "[$Table]" + $newline + "$Key = $Value" + $newline
    }
    $prefix = ''
    if ($insertAt -gt 0 -and $Text[$insertAt - 1] -notin @("`r", "`n")) { $prefix = $newline }
    return $Text.Substring(0, $insertAt) + $prefix + "$Key = $Value" + $newline + $Text.Substring($insertAt)
}

function Get-JcodeEditedConfig($Original, [switch]$WithTheme) {
    $edited = Set-JcodeTomlKey $Original.Text 'features' 'check_updates' 'false' -Boolean
    # Never dump the whole config or even its comments: they can contain secrets.
    Write-Host 'Config diff (key-only, comments and unrelated content redacted):'
    if ($edited -cne $Original.Text) {
        Write-Host '- [features] check_updates = true (or absent/default true)'
        Write-Host '+ [features] check_updates = false'
    } else { Write-Host '  [features] check_updates = false (already disabled)' }
    if ($WithTheme) {
        $themePath = Join-Path $PSScriptRoot '..\..\assets\custom\theme.toml'
        foreach ($line in [IO.File]::ReadAllLines([IO.Path]::GetFullPath($themePath))) {
            if ($line -match '^([a-z_]+)\s*=\s*("#[a-fA-F0-9]{6}")\s*$') {
                $key = $Matches[1]; $value = $Matches[2]
                $next = Set-JcodeTomlKey $edited 'display.colors' $key $value
                if ($next -cne $edited) { Write-Host "+ [display.colors] $key = $value (only this color value merged)" }
                $edited = $next
            } elseif ($line.Trim() -and $line -notmatch '^\s*(#|\[display\.colors\]\s*$)') { throw 'Unexpected content in bundled theme. Nothing changed.' }
        }
    }
    return $edited
}

function Get-JcodeFileHash([string]$Path) {
    Initialize-JcodeFileIO
    [JcodeMainFileIO]::Hash($Path)
}

function Get-JcodeVersion([string]$Path) {
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $Path
    $info.Arguments = '--version'
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    try {
        if (-not $process.WaitForExit(15000)) { throw "--version timed out. Close PID $($process.Id) manually. No process was killed." }
        $version = $process.StandardOutput.ReadToEnd().Trim()
        if ($process.ExitCode -ne 0 -or -not $version -or $version -notmatch '(?i)jcode|cracked') { throw 'Executable failed --version verification.' }
        return $version
    } finally { $process.Dispose() }
}

function Resolve-JcodeArtifact([switch]$DryRun) {
    $repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
    Push-Location $repo
    try {
        $json = & cargo metadata --no-deps --offline --format-version 1
        if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed. Nothing installed.' }
        $metadata = ($json -join "`n") | ConvertFrom-Json
        Write-Host ('Cargo selfdev default candidate: ' + (Join-Path $metadata.target_directory 'selfdev\jcode.exe'))
        Write-Host 'Build/resolve: cargo build --profile selfdev -p jcode --bin jcode --message-format=json-render-diagnostics'
        Write-Host 'Actual source comes from compiler-artifact.executable, including Cargo target overrides.'
        if ($DryRun) { Write-Host 'WhatIf: no Cargo build or executable invocation.'; return }
        $artifact = $null
        & cargo build --profile selfdev -p jcode --bin jcode --message-format=json-render-diagnostics | ForEach-Object {
            try { $message = $_ | ConvertFrom-Json } catch { Write-Host $_; return }
            if ($message.reason -eq 'compiler-artifact' -and $message.target.name -eq 'jcode' -and $message.executable) { $artifact = $message.executable }
            if ($message.reason -eq 'compiler-message' -and $message.message.rendered) { Write-Host $message.message.rendered }
        }
        if ($LASTEXITCODE -ne 0 -or -not $artifact) { throw 'Cargo failed or did not report a jcode executable. Nothing installed.' }
        return [IO.Path]::GetFullPath($artifact)
    } finally { Pop-Location }
}

if ($MyInvocation.InvocationName -eq '.') { return }
$paths = Get-JcodeMainPaths
if ($env:JCODE_CHECK_UPDATES -and $env:JCODE_CHECK_UPDATES.Trim() -match '^(?i:true|1|yes|on)$') {
    throw 'Unset JCODE_CHECK_UPDATES first: it overrides features.check_updates. Do not set it to true in future shells.'
}
Assert-JcodeMainStopped $paths -WaitForClose:$Force -Seconds $WaitSeconds -DryRun:$WhatIfPreference
$original = Read-JcodeConfig $paths.Config
$edited = Get-JcodeEditedConfig $original -WithTheme:$Theme
$stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffffffZ')
$exeBackup = $paths.Exe + '.stock-' + $stamp
$configBackup = $paths.Config + '.bak-' + $stamp
Write-Host "Main executable: $($paths.Exe)"
Write-Host "Existing home retained: $(Split-Path $paths.Config) (no auth/session/settings migration or re-auth)"
Write-Host "Backup executable: $exeBackup"
Write-Host "Backup config ONLY: $configBackup"
Write-Host 'Plan: resolve fork; verify source --version; recheck processes and hashes; back up config and executable;'
Write-Host '      disable check_updates (and optional palette only); stage and atomically replace jcode.exe; verify hash and --version.'
Write-Host 'On verification/write failure: restore this backup pair automatically. Auth files are never opened, copied, or modified.'
Write-Host 'No PATH, JCODE_HOME, environment persistence, channel promotion, or runtime changes. Avoid manual update/install/selfdev/reload commands.'
Write-Host ('Rollback command: powershell -NoProfile -ExecutionPolicy Bypass -File "' + (Join-Path $PSScriptRoot 'rollback-main.ps1') + '"')
$source = $null
if ($ArtifactPath) {
    $source = [IO.Path]::GetFullPath($ArtifactPath)
    Write-Host "Prebuilt source: $source"
} elseif ($WhatIfPreference) { Resolve-JcodeArtifact -DryRun }
if (-not $PSCmdlet.ShouldProcess($paths.Exe, 'Back up main executable/config, disable updates, and install fork with verification')) { return }
$oldExeHash = Get-JcodeFileHash $paths.Exe
$oldConfigHash = [JcodeMainFileIO]::HashBytes($original.Bytes)
if (-not $source) { $source = Resolve-JcodeArtifact }
Assert-JcodePlainPath $source
if ([IO.Path]::GetExtension($source) -ine '.exe' -or $source -ieq $paths.Exe -or -not (Test-Path -LiteralPath $source -PathType Leaf)) { throw 'Source must be a distinct existing Windows .exe.' }
$sourceHash = Get-JcodeFileHash $source
$sourceVersion = Get-JcodeVersion $source
Assert-JcodeMainStopped $paths -WaitForClose:$Force -Seconds $WaitSeconds
if ((Get-JcodeFileHash $paths.Exe) -ne $oldExeHash -or (Get-JcodeFileHash $paths.Config) -ne $oldConfigHash) { throw 'Main files changed during preflight/build. Retry without concurrent config editing.' }
foreach ($path in @($paths.Exe, $paths.Config, $exeBackup, $configBackup)) { Assert-JcodePlainPath $path }
if ((Test-Path -LiteralPath $exeBackup) -or (Test-Path -LiteralPath $configBackup)) { throw 'Backup name collision. Retry.' }
# Config first. A binary backup marks a complete paired backup, never an orphan.
[IO.File]::Copy($paths.Config, $configBackup, $false)
[IO.File]::Copy($paths.Exe, $exeBackup, $false)
$stage = Join-Path (Split-Path $paths.Exe) ('jcode-stage-' + [Guid]::NewGuid().ToString('N') + '.exe')
$changed = $false
try {
    if ((Get-JcodeFileHash $exeBackup) -ne $oldExeHash -or (Get-JcodeFileHash $configBackup) -ne $oldConfigHash) { throw 'Backup integrity check failed.' }
    [IO.File]::Copy($source, $stage, $false)
    if ((Get-JcodeFileHash $stage) -ne $sourceHash) { throw 'Staged executable hash differs from source.' }
    Assert-JcodeMainStopped $paths
    if ((Get-JcodeFileHash $paths.Exe) -ne $oldExeHash -or (Get-JcodeFileHash $paths.Config) -ne $oldConfigHash) { throw 'Main files changed during staging. Nothing replaced.' }
    $changed = $true
    [IO.File]::WriteAllBytes($paths.Config, (ConvertTo-JcodeConfigBytes $original $edited))
    [IO.File]::Replace($stage, $paths.Exe, [NullString]::Value)
    if ((Get-JcodeFileHash $paths.Exe) -ne $sourceHash) { throw 'Installed executable hash mismatch.' }
    $installedVersion = Get-JcodeVersion $paths.Exe
    if ($installedVersion -cne $sourceVersion) { throw 'Installed --version differs from source.' }
    Write-Host "Installed and verified: $installedVersion"
} catch {
    $failure = $_
    if ($changed) {
        [IO.File]::Copy($exeBackup, $paths.Exe, $true)
        [IO.File]::Copy($configBackup, $paths.Config, $true)
        Write-Host 'Restored executable and config from this transaction after failure.'
    }
    throw $failure
} finally {
    if (Test-Path -LiteralPath $stage) { [IO.File]::Delete($stage) }
}
