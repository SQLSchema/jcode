[CmdletBinding()]
param([switch]$RealDryRunOnly)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$apply = Join-Path $PSScriptRoot 'apply-to-main.ps1'
$rollback = Join-Path $PSScriptRoot 'rollback-main.ps1'
function Assert-Test([bool]$Condition, [string]$Message) { if (-not $Condition) { throw "ASSERTION FAILED: $Message" } }
function Get-TestSnapshot([string[]]$Paths) {
    $result = @{}
    foreach ($path in $Paths) {
        $hash = Get-JcodeFileHash $path
        $file = Get-Item -LiteralPath $path -Force
        $result[$path] = "$hash/$($file.Length)/$($file.CreationTimeUtc.Ticks)/$($file.LastWriteTimeUtc.Ticks)/$($file.LastAccessTimeUtc.Ticks)"
    }
    return $result
}
function Assert-TestSnapshot($Before, [string]$Message) {
    $after = Get-TestSnapshot @($Before.Keys)
    foreach ($path in $Before.Keys) {
        if ($Before[$path] -cne $after[$path]) {
            $left = $Before[$path].Split('/'); $right = $after[$path].Split('/')
            $labels = @('SHA256', 'size', 'creation', 'write', 'access')
            $changedFields = @(for ($i = 0; $i -lt $labels.Count; $i++) { if ($left[$i] -cne $right[$i]) { $labels[$i] } }) -join ', '
            throw "ASSERTION FAILED: $Message (file: $([IO.Path]::GetFileName($path)), changed fields: $changedFields)"
        }
    }
}
function Assert-TestRefuses([scriptblock]$Action, [string]$Message) {
    $refused = $false
    try { & $Action } catch { $refused = $true }
    Assert-Test $refused $Message
}
function Get-TestHash([string]$Path) { Get-JcodeFileHash $Path }

foreach ($script in @($apply, $rollback, $PSCommandPath)) {
    $tokens = $null; $parseErrors = $null
    [Management.Automation.Language.Parser]::ParseFile($script, [ref]$tokens, [ref]$parseErrors) | Out-Null
    Assert-Test ($parseErrors.Count -eq 0) "PowerShell syntax: $script"
}
. $apply
Initialize-JcodeFileIO
if ($RealDryRunOnly) {
    # The only real-home read is config itself for surgical planning and hashing.
    # No real auth/credential file is read, listed, copied, or changed by this test.
    $realExe = Join-Path $env:LOCALAPPDATA 'jcode\bin\jcode.exe'
    $realConfig = Join-Path $env:USERPROFILE '.jcode\config.toml'
    $before = Get-TestSnapshot @($realExe, $realConfig)
    $beforeNames = (@(Get-ChildItem -LiteralPath (Split-Path $realExe) -Name | Sort-Object) -join "`n") + "`n" +
        (@(Get-ChildItem -LiteralPath (Split-Path $realConfig) -Filter 'config.toml*' -Name | Sort-Object) -join "`n")
    & $apply -WhatIf
    Assert-TestSnapshot $before 'Real executable/config hash, size, and all timestamps unchanged by WhatIf'
    $afterNames = (@(Get-ChildItem -LiteralPath (Split-Path $realExe) -Name | Sort-Object) -join "`n") + "`n" +
        (@(Get-ChildItem -LiteralPath (Split-Path $realConfig) -Filter 'config.toml*' -Name | Sort-Object) -join "`n")
    Assert-Test ($beforeNames -ceq $afterNames) 'Real WhatIf creates no backups/staged files'
    Write-Host 'PASS: REAL -WhatIf printed plan. Executable/config SHA256, sizes, creation/write/access timestamps and backup inventory unchanged.'
    return
}

$temp = Join-Path ([IO.Path]::GetTempPath()) ('jcode-main-test-' + [Guid]::NewGuid().ToString('N'))
$savedEnv = @{}
$envNames = @('LOCALAPPDATA', 'USERPROFILE', 'JCODE_HOME', 'JCODE_INSTALL_DIR', 'JCODE_CHECK_UPDATES')
foreach ($name in $envNames) { $savedEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
$held = $null
try {
    [IO.Directory]::CreateDirectory($temp) | Out-Null
    $env:LOCALAPPDATA = Join-Path $temp 'Local AppData'
    $env:USERPROFILE = Join-Path $temp 'User Profile'
    foreach ($name in @('JCODE_HOME', 'JCODE_INSTALL_DIR', 'JCODE_CHECK_UPDATES')) { [Environment]::SetEnvironmentVariable($name, $null, 'Process') }
    $bin = Join-Path $env:LOCALAPPDATA 'jcode\bin'
    $homeDir = Join-Path $env:USERPROFILE '.jcode'
    foreach ($dir in @($bin, $homeDir, (Join-Path $homeDir 'sessions'), (Join-Path $env:USERPROFILE '.claude'), (Join-Path $env:USERPROFILE '.codex'))) { [IO.Directory]::CreateDirectory($dir) | Out-Null }
    $exe = Join-Path $bin 'jcode.exe'
    $config = Join-Path $homeDir 'config.toml'
    $fork = Join-Path $temp 'fork.exe'
    $badFork = Join-Path $temp 'bad-fork.exe'
    # Harmless tiny executables: no networking, login, update, or config access.
    foreach ($fixture in @(@('Stock', $exe, 'jcode sandbox-stock', 'false'), @('Fork', $fork, 'jcode sandbox-fork', 'false'), @('Bad', $badFork, 'jcode sandbox-bad', 'true'))) {
        $code = @"
using System;
using System.IO;
using System.Threading;
public static class $($fixture[0]) {
    public static int Main(string[] args) {
        if (args.Length == 1 && args[0] == "--hold") { Thread.Sleep(8000); return 0; }
        if (args.Length != 1 || args[0] != "--version") return 7;
        bool failInstalled = $($fixture[3]);
        if (failInstalled && Path.GetFileName(Environment.GetCommandLineArgs()[0]).Equals("jcode.exe", StringComparison.OrdinalIgnoreCase)) return 9;
        Console.WriteLine("$($fixture[2])"); return 0;
    }
}
"@
        Add-Type -TypeDefinition $code -OutputAssembly $fixture[1] -OutputType ConsoleApplication | Out-Null
    }
    # Sanitized sample from literal fixture text, never copied from the real home.
    $sample = @'
# sanitized, auth-free config fixture
model = "keep-model"
matrix = [
  [1, 2],
  [3, 4],
]
prompt = """
[features]
check_updates = true
"""
[features] # keep this comment
  check_updates  =  true   # preserve this comment too
swarm = true
update_channel = "preview"
[display]
theme = "dark"
[display.colors]
accent = "#010203" # keep palette comment
custom_role = "#abcdef"
[auth]
trusted_external_source_paths = ["C:/SANITIZED/.claude/.credentials.json", "C:/SANITIZED/.codex/auth.json"]
'@
    $sample = $sample.Replace("`r`n", "`n").Replace("`n", "`r`n") # CRLF, no trailing newline
    $utf8 = New-Object Text.UTF8Encoding($false)
    [IO.File]::WriteAllText($config, $sample, $utf8)
    $untouched = @((Join-Path $homeDir 'auth.json'), (Join-Path $homeDir 'openai-auth.json'), (Join-Path $homeDir 'sessions\fixture.json'), (Join-Path $env:USERPROFILE '.claude\.credentials.json'), (Join-Path $env:USERPROFILE '.codex\auth.json'))
    foreach ($path in $untouched) { [IO.File]::WriteAllText($path, '{"fixture":"NOT-A-CREDENTIAL"}', $utf8) }
    $authBefore = Get-TestSnapshot $untouched
    $originalExeHash = Get-TestHash $exe
    $originalConfigHash = Get-TestHash $config
    $before = Get-TestSnapshot @(Get-ChildItem -LiteralPath $temp -File -Recurse | ForEach-Object FullName)
    $count = @(Get-ChildItem -LiteralPath $temp -File -Recurse).Count
    & $apply -ArtifactPath $fork -Theme -WhatIf
    Assert-TestSnapshot $before 'Sandbox WhatIf leaves all file hashes/timestamps unchanged'
    Assert-Test (@(Get-ChildItem -LiteralPath $temp -File -Recurse).Count -eq $count) 'Sandbox WhatIf creates no files'

    & $apply -ArtifactPath $fork
    Assert-Test ((Get-TestHash $exe) -eq (Get-TestHash $fork)) 'Binary really swapped'
    $expected = $sample.Replace('  check_updates  =  true   # preserve', '  check_updates  =  false   # preserve')
    Assert-Test ((Read-JcodeConfig $config).Text -ceq $expected) 'Only the actual update boolean changed, preserving multiline text/comments/formatting'
    Assert-TestSnapshot $authBefore 'Auth/external credentials/sessions untouched after apply'
    Start-Sleep -Seconds 2 # allow the just-executed fixture loader's access-time update to settle
    $applied = Get-TestSnapshot @($exe, $config)
    & $rollback -WhatIf
    Assert-TestSnapshot $applied 'Rollback WhatIf is read-only'

    # Main-install process gate, including -Force timeout. Test process exits naturally.
    $held = Start-Process -FilePath $exe -ArgumentList '--hold' -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 2
    $runningSnapshot = Get-TestSnapshot @($exe, $config)
    Assert-TestRefuses { & $apply -ArtifactPath $fork } 'Running main process refuses apply'
    Assert-TestRefuses { & $apply -ArtifactPath $fork -Force -WaitSeconds 1 } 'Force never bypasses a running process'
    Assert-TestRefuses { & $rollback } 'Running main process refuses rollback'
    Assert-Test (-not $held.HasExited) 'Scripts did not kill the test process'
    Assert-TestSnapshot $runningSnapshot 'Refused operations leave files untouched'
    Assert-Test ($held.WaitForExit(15000)) 'Fixture exits naturally'
    $held.Dispose(); $held = $null
    & $rollback
    Assert-Test ((Get-TestHash $exe) -eq $originalExeHash -and (Get-TestHash $config) -eq $originalConfigHash) 'Rollback restores byte-identical stock files'

    & $apply -ArtifactPath $fork -Theme
    $themed = (Read-JcodeConfig $config).Text
    $expectedTheme = $expected.Replace('accent = "#010203"', 'accent = "#3d5afe"')
    # Exactly bundled roles added within [display.colors], no unrelated content changed.
    $addedRoles = @()
    foreach ($line in [IO.File]::ReadAllLines((Join-Path $PSScriptRoot '..\..\assets\custom\theme.toml'))) {
        if ($line -match '^([a-z_]+)\s*=\s*("#[a-fA-F0-9]{6}")\s*$' -and $Matches[1] -ne 'accent') { $addedRoles += ($Matches[1] + ' = ' + $Matches[2]) }
    }
    # Each absent role is inserted immediately before the next table.
    $expectedTheme = $expectedTheme.Replace('[auth]', (($addedRoles -join "`r`n") + "`r`n[auth]"))
    Assert-Test ($themed -ceq $expectedTheme) 'Theme changes display.colors only, preserving custom role and all other bytes'
    & $rollback
    Assert-Test ((Get-TestHash $exe) -eq $originalExeHash -and (Get-TestHash $config) -eq $originalConfigHash) 'Theme rollback byte-identical'

    # Installation verifies the copied executable, not only the source executable.
    Assert-TestRefuses { & $apply -ArtifactPath $badFork } 'Installed --version failure causes automatic recovery'
    Assert-Test ((Get-TestHash $exe) -eq $originalExeHash -and (Get-TestHash $config) -eq $originalConfigHash) 'Failed install automatically restores byte-identical originals'
    # Newest pairing is filename-based even when a newer config-only orphan exists.
    $orphan = $config + '.bak-99991231T2359599999999Z'
    [IO.File]::WriteAllText($orphan, 'orphan fixture', $utf8)
    & $rollback
    Assert-Test ((Get-TestHash $config) -eq $originalConfigHash) 'Rollback ignores unmatched config-only orphan'
    [IO.File]::Delete($orphan)
    $orphanExe = $exe + '.stock-99991231T2359599999999Z'
    [IO.File]::Copy($exe, $orphanExe)
    Assert-TestRefuses { & $rollback } 'Incomplete newest binary pair refuses rollback'
    [IO.File]::Delete($orphanExe)

    # UTF-8 BOM, UTF-16 BOM, missing table/key, quoted keys and LF are preserved.
    foreach ($encoding in @((New-Object Text.UTF8Encoding($true)), (New-Object Text.UnicodeEncoding($false, $true)))) {
        $fixtureText = "[features]`n`"check_updates`" = true # keep`n[display]`ntheme = 'dark'"
        [IO.File]::WriteAllText($config, $fixtureText, $encoding)
        $hash = Get-TestHash $config
        & $apply -ArtifactPath $fork
        $want = $encoding.GetPreamble() + $encoding.GetBytes($fixtureText.Replace('= true # keep', '= false # keep'))
        Assert-Test ([Convert]::ToBase64String([JcodeMainFileIO]::Read($config)) -ceq [Convert]::ToBase64String($want)) 'BOM/encoding/newlines preserved'
        & $rollback
        Assert-Test ((Get-TestHash $config) -eq $hash) 'Encoded config rollback byte-identical'
    }
    foreach ($fixtureText in @("[display]`ntheme = 'dark'", "[features]`nswarm = true`n[display]`ntheme = 'dark'")) {
        [IO.File]::WriteAllText($config, $fixtureText, $utf8)
        $hash = Get-TestHash $config
        & $apply -ArtifactPath $fork
        Assert-Test ((Read-JcodeConfig $config).Text -match '(?m)^check_updates = false$') 'Absent update key/table inserted'
        & $rollback
        Assert-Test ((Get-TestHash $config) -eq $hash) 'Inserted key rollback byte-identical'
    }
    [IO.File]::WriteAllText($config, "[features]`ncheck_updates = true`ncheck_updates = false", $utf8)
    $hash = Get-TestHash $config
    Assert-TestRefuses { & $apply -ArtifactPath $fork } 'Duplicate target key refuses before writes'
    Assert-Test ((Get-TestHash $config) -eq $hash) 'Invalid config not modified'
    foreach ($fixtureText in @(
        "features = { check_updates = true }",
        "'features'.check_updates = true",
        "[features]`ncheck_updates = true`n[features]`nswarm = true",
        "[[features]]`ncheck_updates = true"
    )) {
        [IO.File]::WriteAllText($config, $fixtureText, $utf8)
        $hash = Get-TestHash $config
        Assert-TestRefuses { & $apply -ArtifactPath $fork } 'Ambiguous target table refuses before writes'
        Assert-Test ((Get-TestHash $config) -eq $hash) 'Ambiguous target config preserved'
    }
    [IO.File]::WriteAllText($config, "[features]`ncheck_updates = true`n[display]`ncolors = { accent = '#010203' }", $utf8)
    $hash = Get-TestHash $config
    Assert-TestRefuses { & $apply -ArtifactPath $fork -Theme } 'Inline palette refuses rather than adding duplicate table'
    Assert-Test ((Get-TestHash $config) -eq $hash) 'Inline palette config preserved'
    [IO.File]::WriteAllText($config, $sample, $utf8)
    $env:JCODE_HOME = $homeDir
    Assert-TestRefuses { & $apply -ArtifactPath $fork } 'JCODE_HOME external-auth redirection refused'
    [Environment]::SetEnvironmentVariable('JCODE_HOME', $null, 'Process')
    $env:JCODE_CHECK_UPDATES = 'true'
    Assert-TestRefuses { & $apply -ArtifactPath $fork } 'Environment update override refused'
    [Environment]::SetEnvironmentVariable('JCODE_CHECK_UPDATES', $null, 'Process')
    Assert-TestSnapshot $authBefore 'All synthetic auth/external credential/session files untouched across every test'
    Write-Host 'PASS: sandbox apply/rollback, surgical config and theme edits, hashes, encodings, process refusal/Force, failure recovery, newest backup pairing, and credential/session preservation.'
} finally {
    if ($held) { $held.WaitForExit(15000) | Out-Null; $held.Dispose() }
    foreach ($name in $envNames) { [Environment]::SetEnvironmentVariable($name, $savedEnv[$name], 'Process') }
    # Only the uniquely created fixture directory is removed, never a real home/install.
    if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Recurse -Force }
}
