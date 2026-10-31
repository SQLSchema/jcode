[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message" }
}
foreach ($file in Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.ps1') {
    $tokens = $null
    $errors = $null
    [Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$errors) | Out-Null
    Assert-True ($errors.Count -eq 0) "PowerShell syntax: $($file.Name): $errors"
}
Write-Host 'PASS: PowerShell syntax'

$palette = [IO.File]::ReadAllText((Join-Path $repoRoot 'crates\jcode-tui-style\src\palette.rs'))
$validKeys = @([regex]::Matches($palette, 'Role::\w+ => "([a-z_]+)"') | ForEach-Object { $_.Groups[1].Value })
$theme = [IO.File]::ReadAllText((Join-Path $repoRoot 'assets\custom\theme.toml'))
$themeKeys = @()
foreach ($line in ($theme -split '\r?\n')) {
    $trimmed = $line.Trim()
    if (-not $trimmed -or $trimmed.StartsWith('#') -or $trimmed -eq '[display.colors]') { continue }
    Assert-True ($trimmed -match '^([a-z_]+)\s*=\s*"#[0-9a-fA-F]{6}"$') "Only RGB color settings in theme: $trimmed"
    $key = $Matches[1]
    Assert-True ($validKeys -contains $key) "Known palette role: $key"
    Assert-True ($themeKeys -notcontains $key) "Unique palette role: $key"
    $themeKeys += $key
}
Assert-True ($validKeys.Count -eq 22 -and $themeKeys.Count -eq $validKeys.Count) 'All 22 palette roles covered'
Write-Host 'PASS: all 22 theme keys and RGB values match palette.rs'

$scratchBase = if ($env:JCODE_SCRATCH_DIR) { $env:JCODE_SCRATCH_DIR } else { [IO.Path]::GetTempPath() }
$testRoot = Join-Path $scratchBase ('cracked packaging & spaces (' + [Guid]::NewGuid().ToString('N') + ')')
$previousLocalAppData = $env:LOCALAPPDATA
$previousHome = $env:JCODE_HOME
$previousSocket = $env:JCODE_SOCKET
$previousRuntime = $env:JCODE_RUNTIME_DIR
try {
    New-Item -ItemType Directory -Path $testRoot -Force | Out-Null
    $env:LOCALAPPDATA = $testRoot
    # Bogus inherited routing must not survive the launcher's setlocal scope.
    $env:JCODE_HOME = 'inherited-stock-home-sentinel'
    $env:JCODE_SOCKET = 'inherited-stock-socket-sentinel'
    $env:JCODE_RUNTIME_DIR = 'inherited-stock-runtime-sentinel'
    $forkRoot = Join-Path $testRoot 'jcode-cracked'
    $configPath = Join-Path $forkRoot 'home\config.toml'

    & (Join-Path $PSScriptRoot 'build.ps1') -WhatIf
    Assert-True (-not (Test-Path -LiteralPath $forkRoot)) 'Build WhatIf makes no install/state directories'
    $output = & (Join-Path $PSScriptRoot 'cracked.cmd') --dry-run 2>&1
    Assert-True ($LASTEXITCODE -eq 0) 'Repo launcher dry run succeeds'
    $outputText = $output -join "`n"
    Assert-True ($outputText.Contains((Join-Path $forkRoot 'run\cracked.sock'))) 'Dry run selects isolated socket'
    Assert-True (-not $outputText.Contains('inherited-stock-')) 'Inherited routing is replaced'
    Assert-True (-not (Test-Path -LiteralPath $forkRoot)) 'Launcher dry run creates no state'
    Assert-True ($env:JCODE_HOME -eq 'inherited-stock-home-sentinel') 'Launcher setlocal preserves parent environment'
    Write-Host 'PASS: build and launcher dry runs, inherited routing, spaces and metacharacters'

    & (Join-Path $PSScriptRoot 'seed.ps1')
    Assert-True (Test-Path -LiteralPath $configPath -PathType Leaf) 'First seed creates fork config'
    $seeded = [IO.File]::ReadAllText($configPath)
    Assert-True ($seeded -eq ("[display]`r`ntheme = `"dark`"`r`n`r`n" + $theme)) 'Seed contains only dark display settings and bundled theme'
    $edited = $seeded + "`r`n# local customization sentinel`r`n"
    [IO.File]::WriteAllText($configPath, $edited)
    & (Join-Path $PSScriptRoot 'seed.ps1')
    Assert-True ([IO.File]::ReadAllText($configPath) -eq $edited) 'Subsequent seed preserves existing config byte content'
    Assert-True (@(Get-ChildItem -LiteralPath (Split-Path $configPath -Parent) -File).Count -eq 1) 'No auth or other files copied'
    Write-Host 'PASS: first-run auth-free seed and existing-config preservation'

    # Installed layout must find the adjacent theme without the checkout.
    $installedDir = Join-Path $forkRoot 'bin'
    New-Item -ItemType Directory -Path $installedDir -Force | Out-Null
    foreach ($name in @('cracked.cmd', 'seed.ps1')) {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $installedDir $name)
    }
    Copy-Item -LiteralPath (Join-Path $repoRoot 'assets\custom\theme.toml') -Destination (Join-Path $installedDir 'theme.toml')
    $output = & (Join-Path $installedDir 'cracked.cmd') --dry-run 2>&1
    Assert-True ($LASTEXITCODE -eq 0) 'Installed launcher dry run succeeds'
    Assert-True ([IO.File]::ReadAllText($configPath) -eq $edited) 'Installed dry run preserves config'
    Write-Host 'PASS: installed launcher/theme layout'
} finally {
    $env:LOCALAPPDATA = $previousLocalAppData
    $env:JCODE_HOME = $previousHome
    $env:JCODE_SOCKET = $previousSocket
    $env:JCODE_RUNTIME_DIR = $previousRuntime
    if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}
Write-Host 'All cracked packaging tests passed. Stock config/auth were not read.'
