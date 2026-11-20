# Installs ashira-core on Windows from the latest GitHub Release:
#   irm https://raw.githubusercontent.com/Night-stars-1/Ashira/master/install.ps1 | iex
# A specific version: $env:ASHIRA_VERSION = "v0.2.0"; irm ... | iex
# Install directory: $env:ASHIRA_INSTALL_DIR (default %LOCALAPPDATA%\Programs\ashira)
# Kept ASCII-only: Windows PowerShell 5.1 reads BOM-less files in the ANSI code page.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$repo = 'Night-stars-1/Ashira'
$dir = if ($env:ASHIRA_INSTALL_DIR) { $env:ASHIRA_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\ashira' }
$target = Join-Path $dir 'ashira.exe'

if (-not [Environment]::Is64BitOperatingSystem) { throw 'ashira-core only ships for 64-bit Windows.' }

$api = if ($env:ASHIRA_VERSION) { "https://api.github.com/repos/$repo/releases/tags/$($env:ASHIRA_VERSION)" } else { "https://api.github.com/repos/$repo/releases/latest" }
$release = Invoke-RestMethod -Uri $api -Headers @{ 'User-Agent' = 'ashira-install' }
$asset = $release.assets | Where-Object { $_.name -like 'ashira-core-*-windows-x64.zip' } | Select-Object -First 1
if (-not $asset) { throw "Release $($release.tag_name) has no Windows ashira-core build." }

Write-Host "Downloading $($asset.name) ($($release.tag_name))"
$temp = Join-Path ([IO.Path]::GetTempPath()) ('ashira-' + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $temp | Out-Null
try {
    $zip = Join-Path $temp $asset.name
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing

    # Checked against the release's SHA256SUMS.txt when it has one.
    $sums = $release.assets | Where-Object { $_.name -eq 'SHA256SUMS.txt' } | Select-Object -First 1
    if ($sums) {
        $sumsFile = Join-Path $temp 'SHA256SUMS.txt'
        Invoke-WebRequest -Uri $sums.browser_download_url -OutFile $sumsFile -UseBasicParsing
        $pattern = '^([0-9a-fA-F]{64})\s+\*?' + [regex]::Escape($asset.name) + '\s*$'
        $match = Get-Content -Path $sumsFile | Select-String -Pattern $pattern | Select-Object -First 1
        if (-not $match) { throw "SHA256SUMS.txt has no entry for $($asset.name)." }
        $expected = $match.Matches[0].Groups[1].Value.ToLower()
        # .NET directly: Get-FileHash can be missing when PowerShell 7's PSModulePath leaks into 5.1.
        $stream = [IO.File]::OpenRead($zip)
        try { $actual = -join ([Security.Cryptography.SHA256]::Create().ComputeHash($stream) | ForEach-Object { $_.ToString('x2') }) } finally { $stream.Dispose() }
        if ($expected -ne $actual) { throw "Checksum mismatch for $($asset.name)." }
    }

    # .NET directly, for the same PSModulePath reason; ZipFile ships with .NET Framework 4.5+.
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::ExtractToDirectory($zip, (Join-Path $temp 'x'))
    $exe = Get-ChildItem -Path $temp -Recurse -Filter 'ashira.exe' | Select-Object -First 1
    if (-not $exe) { throw 'The archive has no ashira.exe.' }

    if (Get-Process -Name ashira -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $target }) {
        throw "ashira is running from $target. Close it and run the installer again."
    }
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    Copy-Item -Path $exe.FullName -Destination $target -Force
} finally {
    Remove-Item -Path $temp -Recurse -Force -ErrorAction SilentlyContinue
}

# User PATH, read raw so entries like %USERPROFILE%\bin stay unexpanded.
$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
try {
    $path = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $entries = @($path -split ';' | Where-Object { $_ })
    if ($entries -notcontains $dir) {
        $key.SetValue('Path', (($entries + $dir) -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
        # Setting a variable through .NET broadcasts WM_SETTINGCHANGE, so new terminals see the PATH.
        [Environment]::SetEnvironmentVariable('ASHIRA_INSTALL_PING', '1', 'User')
        [Environment]::SetEnvironmentVariable('ASHIRA_INSTALL_PING', $null, 'User')
        Write-Host "Added $dir to your user PATH; open a new terminal to use it."
    }
} finally {
    $key.Close()
}
if (($env:Path -split ';') -notcontains $dir) { $env:Path = "$env:Path;$dir" }

Write-Host "Installed $(& $target --version) to $target"
