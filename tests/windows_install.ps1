param(
  [Parameter(Mandatory=$true)][string]$Installer,
  [Parameter(Mandatory=$true)][string]$Version
)
$ErrorActionPreference = 'Stop'
# GitHub's disposable Windows runner only. Never launch the measurement app.
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Run this lifecycle test on a disposable GitHub Actions runner.' }
$setup = (Resolve-Path $Installer).Path
$install = Join-Path $env:ProgramFiles 'Kohdalab\KohdaLab TRKR'
$registry = 'HKLM:\Software\Kohdalab\KohdaLab TRKR'
$uninstallRegistry = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\KohdaLab TRKR'
if (Test-Path $install) { throw 'Refusing to overwrite a pre-existing installation.' }
$profile = Join-Path $env:USERPROFILE '.kohdalab\trkr\config\config.json'
$legacy = Join-Path $env:USERPROFILE '.kohdalab\config\trkr-next.json'
$marker = '{"lifecycle_test":"preserve user configuration"}'
foreach ($path in @($profile,$legacy)) {
  if (Test-Path $path) { throw "Refusing to overwrite existing settings: $path" }
  New-Item -ItemType Directory -Force (Split-Path $path) | Out-Null
  [IO.File]::WriteAllText($path,$marker)
}
$outside = Join-Path $env:RUNNER_TEMP 'trkr-install-override'
New-Item -ItemType Directory -Force $outside | Out-Null
$outsideMarker = Join-Path $outside 'preserve.txt'
[IO.File]::WriteAllText($outsideMarker,'do not remove')
try {
  $result = Start-Process -FilePath $setup -ArgumentList @('/S',"/D=$outside") -PassThru -Wait
  if ($result.ExitCode -ne 0) { throw "Install failed: $($result.ExitCode)" }
  foreach ($file in @('kohdalab-trkr.exe','kohdalab-trkr-demo.exe','support\kohdalab-gpib-helper.exe','Uninstall.exe')) {
    if (-not (Test-Path (Join-Path $install $file))) { throw "Missing installed file: $file" }
  }
  if (Test-Path (Join-Path $outside 'kohdalab-trkr.exe')) { throw 'Installer accepted the forbidden /D override.' }
  if ((Get-ItemProperty $registry).InstallLocation -ne $install) { throw 'Machine install location differs.' }
  if ((Get-ItemProperty $uninstallRegistry).DisplayVersion -ne $Version) { throw 'Installed version differs.' }
  $desktop = Join-Path ([Environment]::GetFolderPath('CommonDesktopDirectory')) 'KohdaLab TRKR.lnk'
  $startMenu = Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'KohdaLab\TRKR.lnk'
  foreach ($shortcut in @($desktop,$startMenu)) {
    if (-not (Test-Path $shortcut)) { throw "Missing all-user shortcut: $shortcut" }
    $target = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut).TargetPath
    if ($target -ne (Join-Path $install 'kohdalab-trkr.exe')) { throw 'Shortcut points to another installation.' }
  }
  $uninstaller = Join-Path $install 'Uninstall.exe'
  $result = Start-Process -FilePath $uninstaller -ArgumentList @('/S',"_?=$outside") -PassThru -Wait
  if ($result.ExitCode -eq 0) { throw 'Uninstaller accepted an unrelated path.' }
  if (-not (Test-Path (Join-Path $install 'kohdalab-trkr.exe'))) { throw 'Rejected uninstall removed the application.' }
  $result = Start-Process -FilePath $uninstaller -ArgumentList @('/S',"_?=$install") -PassThru -Wait
  if ($result.ExitCode -ne 0) { throw "Uninstall failed: $($result.ExitCode)" }
  if ((Test-Path (Join-Path $install 'kohdalab-trkr.exe')) -or (Test-Path $registry) -or (Test-Path $uninstallRegistry)) { throw 'Application or registry remains after uninstall.' }
  foreach ($shortcut in @($desktop,$startMenu)) {
    if (Test-Path $shortcut) { throw 'Uninstall left a shortcut.' }
  }
  foreach ($path in @($profile,$legacy)) {
    if ([IO.File]::ReadAllText($path) -ne $marker) { throw 'Installation changed user configuration.' }
  }
  if ([IO.File]::ReadAllText($outsideMarker) -ne 'do not remove') { throw 'An unrelated path was changed.' }
  Write-Output 'PASS: system install, fixed paths, shared shortcuts, guarded uninstall and preserved configuration.'
} finally {
  # Delete only the two fixtures created by this test, never operator settings.
  foreach ($path in @($profile,$legacy)) {
    if ((Test-Path $path) -and ([IO.File]::ReadAllText($path) -eq $marker)) { Remove-Item $path }
  }
}
