[CmdletBinding(SupportsShouldProcess = $true)]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not $env:LOCALAPPDATA) { throw 'LOCALAPPDATA is not set.' }
$forkRoot = Join-Path $env:LOCALAPPDATA 'jcode-cracked'
$homePath = Join-Path $forkRoot 'home'
$runtimePath = Join-Path $forkRoot 'run'
$configPath = Join-Path $homePath 'config.toml'
$themePath = Join-Path $PSScriptRoot 'theme.toml'
if (-not (Test-Path -LiteralPath $themePath -PathType Leaf)) {
    $themePath = Join-Path $PSScriptRoot '..\..\assets\custom\theme.toml'
}
if (-not (Test-Path -LiteralPath $themePath -PathType Leaf)) {
    throw "Theme file missing: $themePath. Reinstall with scripts\cracked\build.ps1."
}

# Never read ~/.jcode/config.toml or any authentication files. Only the checked-in
# display palette is used. Reject linked destinations to avoid writing elsewhere.
foreach ($path in @($forkRoot, $homePath, $runtimePath, $configPath)) {
    if (Test-Path -LiteralPath $path) {
        $item = Get-Item -LiteralPath $path -Force
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw "Refusing linked fork state path: $path"
        }
    }
}
foreach ($path in @($homePath, $runtimePath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Container)) {
        if ($PSCmdlet.ShouldProcess($path, 'Create isolated fork directory')) {
            New-Item -ItemType Directory -Path $path -Force | Out-Null
        }
    }
}
if (-not (Test-Path -LiteralPath $configPath)) {
    if ($PSCmdlet.ShouldProcess($configPath, 'Seed auth-free dark display palette')) {
        $contents = "[display]`r`ntheme = `"dark`"`r`n`r`n" + [IO.File]::ReadAllText($themePath)
        # CreateNew handles simultaneous first launches without overwriting edits.
        $stream = [IO.File]::Open($configPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        try {
            $bytes = [Text.UTF8Encoding]::new($false).GetBytes($contents)
            $stream.Write($bytes, 0, $bytes.Length)
        } finally {
            $stream.Dispose()
        }
        Write-Host "Seeded fork display config: $configPath"
    }
}
