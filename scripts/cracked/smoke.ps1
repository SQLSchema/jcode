[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $env:LOCALAPPDATA) { throw 'LOCALAPPDATA is not set.' }
$sourceExe = Join-Path $env:LOCALAPPDATA 'jcode-cracked\bin\cracked.exe'
if (-not (Test-Path -LiteralPath $sourceExe -PathType Leaf)) { throw 'Run scripts/cracked/build.ps1 first.' }
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$stockExe = Join-Path $env:LOCALAPPDATA 'jcode\bin\jcode.exe'
$stockBefore = @(Get-Process -Name jcode -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $stockExe } | ForEach-Object {
    [pscustomobject]@{ Id = $_.Id; Started = $_.StartTime.ToUniversalTime().Ticks }
})
$scratchBase = if ($env:JCODE_SCRATCH_DIR) { $env:JCODE_SCRATCH_DIR } else { [IO.Path]::GetTempPath() }
$testRoot = Join-Path $scratchBase ('cracked-runtime-smoke-' + [Guid]::NewGuid().ToString('N'))
$binDir = Join-Path $testRoot 'jcode-cracked\bin'
$socketPath = Join-Path $testRoot 'jcode-cracked\run\cracked.sock'
$testExe = Join-Path $binDir 'cracked.exe'
$wrapper = $null
$child = $null
$pipe = $null
$reader = $null
try {
    New-Item -ItemType Directory -Path $binDir -Force | Out-Null
    Copy-Item -LiteralPath $sourceExe -Destination $testExe
    foreach ($name in @('cracked.cmd', 'seed.ps1')) {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $binDir $name)
    }
    Copy-Item -LiteralPath (Join-Path $repoRoot 'assets\custom\theme.toml') -Destination (Join-Path $binDir 'theme.toml')
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $env:ComSpec
    $testLauncher = Join-Path $binDir 'cracked.cmd'
    $startInfo.Arguments = '/d /s /c ""' + $testLauncher + '" --no-selfdev --provider openai-compatible --model cracked-packaging-smoke serve"'
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.WorkingDirectory = $testRoot
    # Exercise the real launcher: it must replace deliberately incorrect routing.
    $startInfo.EnvironmentVariables['LOCALAPPDATA'] = $testRoot
    $startInfo.EnvironmentVariables['JCODE_HOME'] = 'inherited-home-must-be-replaced'
    $startInfo.EnvironmentVariables['JCODE_RUNTIME_DIR'] = 'inherited-runtime-must-be-replaced'
    $startInfo.EnvironmentVariables['JCODE_SOCKET'] = 'inherited-socket-must-be-replaced'
    $startInfo.EnvironmentVariables['JCODE_INSTALL_DIR'] = 'inherited-install-must-be-replaced'
    $startInfo.EnvironmentVariables['JCODE_PROVIDER_PROFILE_NAME'] = 'stock-profile-absent-in-fork'
    $startInfo.EnvironmentVariables['JCODE_PROVIDER_PROFILE_ACTIVE'] = '1'
    $startInfo.EnvironmentVariables['JCODE_NAMED_PROVIDER_PROFILE'] = 'stock-profile-absent-in-fork'
    $startInfo.EnvironmentVariables['JCODE_NO_TELEMETRY'] = '1'
    $startInfo.EnvironmentVariables['JCODE_OPENAI_COMPAT_API_BASE'] = 'http://127.0.0.1:9/v1'
    $startInfo.EnvironmentVariables['JCODE_OPENAI_COMPAT_DEFAULT_MODEL'] = 'cracked-packaging-smoke'
    $startInfo.EnvironmentVariables['JCODE_OPENAI_COMPAT_LOCAL_ENABLED'] = '1'
    $startInfo.EnvironmentVariables['JCODE_TEMP_SERVER'] = '1'
    $startInfo.EnvironmentVariables['JCODE_SERVER_OWNER_PID'] = [string]$PID
    $startInfo.EnvironmentVariables.Remove('JCODE_TEST_SESSION')
    $wrapper = [Diagnostics.Process]::Start($startInfo)
    # Drain output without printing runtime logs or potentially sensitive values.
    $stdoutTask = $wrapper.StandardOutput.ReadToEndAsync()
    $stderrTask = $wrapper.StandardError.ReadToEndAsync()

    # Windows transport hashes the full normalized socket path, not just its stem.
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $digest = $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($socketPath.Replace('\', '/').ToLowerInvariant()))
    } finally { $sha.Dispose() }
    $hex = ([BitConverter]::ToString($digest)).Replace('-', '').ToLowerInvariant()
    $pipeName = 'cracked-' + $hex.Substring(0, 16)
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        if ($wrapper.HasExited) {
            $diagnostics = $stdoutTask.Result + $stderrTask.Result
            $labels = @('unknown provider profile', 'unexpected argument', 'No credentials',
                'cannot find', 'Access is denied', 'failed', 'config', 'provider', 'socket') |
                Where-Object { $diagnostics.IndexOf($_, [StringComparison]::OrdinalIgnoreCase) -ge 0 }
            throw "Isolated serve exited before readiness ($($wrapper.ExitCode)). Diagnostic categories ($($diagnostics.Length) chars): $($labels -join ', '). Runtime log values are not printed."
        }
        if (-not $child) {
            $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($wrapper.Id) AND Name = 'cracked.exe'")
            if ($children.Count -gt 0) {
                $candidate = Get-Process -Id $children[0].ProcessId -ErrorAction Stop
                if ($candidate.Path -ne $testExe) { throw 'Refusing unexpected child executable identity.' }
                $child = $candidate
            }
        }
        $pipe = [IO.Pipes.NamedPipeClientStream]::new('.', $pipeName, [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
        try { $pipe.Connect(250) } catch [TimeoutException] { $pipe.Dispose(); $pipe = $null }
    } while (-not $pipe -and [DateTime]::UtcNow -lt $deadline)
    if (-not $pipe -or -not $child) { throw 'Isolated launcher did not create its own named pipe and child process within 30 seconds.' }
    $request = [Text.Encoding]::UTF8.GetBytes('{"type":"ping","id":12001}' + "`n")
    $pipe.Write($request, 0, $request.Length)
    $pipe.Flush()
    $reader = [IO.StreamReader]::new($pipe)
    $pong = $false
    for ($attempt = 0; $attempt -lt 4; $attempt++) {
        $readTask = $reader.ReadLineAsync()
        if (-not $readTask.Wait(5000)) { throw 'Isolated IPC ping timed out.' }
        $event = $readTask.Result | ConvertFrom-Json
        if ($event.type -eq 'pong' -and $event.id -eq 12001) { $pong = $true; break }
    }
    if (-not $pong) { throw 'No matching pong received from the isolated server.' }
    Write-Host "PASS: real launcher started its own server PID $($child.Id) and returned matching IPC pong on the isolated named pipe"

    $configPath = Join-Path $testRoot 'jcode-cracked\home\config.toml'
    & python -c "import pathlib,sys,tomllib; c=tomllib.loads(pathlib.Path(sys.argv[1]).read_text(encoding='utf-8')); assert set(c)=={'display'}; assert set(c['display'])=={'theme','colors'}; assert c['display']['theme']=='dark'; assert len(c['display']['colors'])==22; print('PASS: actual first-run config parses as TOML with only dark display settings and all 22 colors')" $configPath
    if ($LASTEXITCODE -ne 0) { throw 'Seeded config TOML validation failed.' }
    foreach ($stock in $stockBefore) {
        $current = Get-Process -Id $stock.Id -ErrorAction Stop
        if ($current.Path -ne $stockExe -or $current.StartTime.ToUniversalTime().Ticks -ne $stock.Started) {
            throw 'Stock process identity changed during runtime smoke test.'
        }
    }
    Write-Host "PASS: $($stockBefore.Count) existing stock jcode process identities/start times unchanged"
} finally {
    if ($reader) { $reader.Dispose() }
    if ($pipe) { $pipe.Dispose() }
    # Stop only the exact child created by this test, never a process by name.
    if (-not $child -and $wrapper) {
        $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($wrapper.Id) AND Name = 'cracked.exe'")
        foreach ($entry in $children) {
            $candidate = Get-Process -Id $entry.ProcessId -ErrorAction SilentlyContinue
            if ($candidate -and $candidate.Path -eq $testExe) { $candidate.Kill(); $candidate.WaitForExit(5000) | Out-Null }
        }
    }
    if ($child -and -not $child.HasExited) { $child.Kill(); $child.WaitForExit(5000) | Out-Null }
    if ($wrapper -and -not $wrapper.HasExited) {
        if (-not $wrapper.WaitForExit(5000)) { $wrapper.Kill(); $wrapper.WaitForExit(5000) | Out-Null }
    }
    if ($wrapper) { $wrapper.Dispose() }
    if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}
Write-Host 'Runtime smoke passed. No provider request was sent and no stock config/auth file was read.'
