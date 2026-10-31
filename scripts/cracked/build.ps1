[CmdletBinding(SupportsShouldProcess = $true)]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $env:LOCALAPPDATA) { throw 'LOCALAPPDATA is not set.' }
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$installDir = Join-Path $env:LOCALAPPDATA 'jcode-cracked\bin'
$installExe = Join-Path $installDir 'cracked.exe'
$launcher = Join-Path $installDir 'cracked.cmd'

Push-Location $repoRoot
try {
    # Metadata reflects CARGO_TARGET_DIR and Cargo config, not an assumed target/.
    $metadataJson = & cargo metadata --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed ($LASTEXITCODE)." }
    $metadata = ($metadataJson -join "`n") | ConvertFrom-Json
    $defaultOutput = Join-Path $metadata.target_directory 'selfdev\jcode.exe'
    Write-Host 'Build command: cargo build --profile selfdev -p jcode --bin jcode --message-format=json-render-diagnostics'
    Write-Host "Default selfdev output: $defaultOutput"
    Write-Host 'The compiler-artifact executable field determines the actual install source (including target overrides).'

    if (-not $PSCmdlet.ShouldProcess($repoRoot, 'Build selfdev jcode and install isolated cracked package')) {
        Write-Host "Would install executable: $installExe"
        Write-Host "Would install launcher: $launcher"
        return
    }
    # The repository config is portable, so no manual --config override is needed.
    # Stamp the real commit: jcode-build-meta deliberately does not watch .git,
    # so without this the footer/--version keep showing a stale hash. Setting
    # these env vars only reruns that tiny build script when the hash changes.
    $gitHash = (& git rev-parse --short=9 HEAD 2>$null)
    if ($LASTEXITCODE -eq 0 -and $gitHash) {
        $env:JCODE_BUILD_GIT_HASH = $gitHash.Trim()
        $dirty = (& git status --porcelain --untracked-files=no 2>$null)
        $env:JCODE_BUILD_GIT_DIRTY = if ($dirty) { '1' } else { '0' }
    }
    # Read Cargo's artifact stream to handle custom target dirs / target triples.
    $artifactPath = $null
    & cargo build --profile selfdev -p jcode --bin jcode --message-format=json-render-diagnostics | ForEach-Object {
        $line = $_
        try { $message = $line | ConvertFrom-Json } catch {
            Write-Host $line
            return
        }
        if ($message.reason -eq 'compiler-artifact' -and $message.target.name -eq 'jcode' -and $message.executable) {
            $artifactPath = $message.executable
        }
        if ($message.reason -eq 'compiler-message' -and $message.message.rendered) {
            Write-Host $message.message.rendered
        }
    }
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE). Nothing was installed." }
    if (-not $artifactPath -or -not (Test-Path -LiteralPath $artifactPath -PathType Leaf)) {
        throw 'Cargo succeeded but did not report a jcode executable artifact. Nothing was installed.'
    }
    if ([IO.Path]::GetExtension($artifactPath) -ne '.exe') {
        throw 'The artifact is not a Windows executable. Remove cross-compilation target overrides.'
    }
    foreach ($path in @((Split-Path $installDir -Parent), $installDir, $installExe, $launcher)) {
        if (Test-Path -LiteralPath $path) {
            if ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw "Refusing linked installation path: $path"
            }
        }
    }
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    # Stage before replacement. Windows refuses to overwrite cracked.exe while
    # it runs, but does allow renaming it, so a running copy is moved aside to
    # cracked.old-*.exe (cleaned up on later builds once it has exited). Never
    # kill processes, especially the stock jcode server.
    Get-ChildItem -LiteralPath $installDir -Filter 'cracked.old-*.exe' -ErrorAction SilentlyContinue |
        ForEach-Object { Remove-Item -LiteralPath $_.FullName -ErrorAction SilentlyContinue }
    $stagedExe = Join-Path $installDir ('cracked-' + [Guid]::NewGuid().ToString('N') + '.exe')
    try {
        Copy-Item -LiteralPath $artifactPath -Destination $stagedExe
        try {
            Move-Item -LiteralPath $stagedExe -Destination $installExe -Force -ErrorAction Stop
        } catch [System.IO.IOException] {
            $aside = Join-Path $installDir ('cracked.old-' + [Guid]::NewGuid().ToString('N').Substring(0, 8) + '.exe')
            Rename-Item -LiteralPath $installExe -NewName (Split-Path -Leaf $aside) -ErrorAction Stop
            Move-Item -LiteralPath $stagedExe -Destination $installExe -ErrorAction Stop
            Write-Host "cracked.exe was running: old copy moved aside; restart open windows to pick up the new build."
        }
    } finally {
        if (Test-Path -LiteralPath $stagedExe) { Remove-Item -LiteralPath $stagedExe }
    }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'cracked.cmd') -Destination $launcher -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'seed.ps1') -Destination (Join-Path $installDir 'seed.ps1') -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'assets\custom\theme.toml') -Destination (Join-Path $installDir 'theme.toml') -Force
    Write-Host "Installed executable: $installExe"
    Write-Host "Run from PowerShell: & `"$launcher`""
    Write-Host "Run from cmd.exe: `"$launcher`""
    Write-Host 'Always use cracked.cmd, not cracked.exe, to keep stock runtime state isolated.'
} finally {
    Pop-Location
}
