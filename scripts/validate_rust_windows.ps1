param(
    [Parameter(Mandatory=$true)][string]$PackageDir,
    [string]$OutputDir
)

$ErrorActionPreference = 'Stop'
$PackageDir = (Resolve-Path -LiteralPath $PackageDir).Path
$binary = Join-Path $PackageDir 'kohdalab-trkr.exe'
if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw "Missing executable: $binary" }
if (-not $OutputDir) {
    $OutputDir = Join-Path $PackageDir ('validation-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff'))
}
if (Test-Path -LiteralPath $OutputDir) { throw "Refusing existing validation output: $OutputDir" }
New-Item -ItemType Directory -Path $OutputDir | Out-Null
$OutputDir = (Resolve-Path -LiteralPath $OutputDir).Path

# Validate exactly the package that is about to run. No hardware is opened.
$manifest = Get-Content -LiteralPath (Join-Path $PackageDir 'SHA256.json') -Raw | ConvertFrom-Json
foreach ($entry in $manifest.PSObject.Properties) {
    $path = Join-Path $PackageDir $entry.Name
    $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    if ($actual -ne $entry.Value) { throw "Package checksum mismatch: $($entry.Name)" }
}

$expectedFields = @(
    'run_id','point_index','timestamp','elapsed_s','quality_flags','measurement','fast_axis','slow_axis',
    'line_index','target_elapsed_s','target_t_cor_ps','target_x_cor_um','target_y_cor_um',
    'target_u_cor_um','target_v_cor_um','t_ps','x_um','y_um','t_cor_ps','x_cor_um','y_cor_um',
    'u_cor_um','v_cor_um','spatial_theta_deg','X','Y','R','Theta_deg','signal_unit','power_W'
)
$results = @()
foreach ($workflow in @('signal_monitor','trkr','srkr','strkr','srkr_2d')) {
    $path = Join-Path $OutputDir ($workflow + '.csv')
    $arguments = @('--demo-csv', ('"' + $path + '"'), '--workflow', $workflow)
    $process = Start-Process -FilePath $binary -ArgumentList $arguments -WorkingDirectory $PackageDir -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "$workflow failed: exit $($process.ExitCode)" }
    $rows = @(Import-Csv -LiteralPath $path)
    if ($rows.Count -eq 0) { throw "$workflow produced no rows" }
    $fields = @($rows[0].PSObject.Properties.Name)
    if (($fields -join ',') -cne ($expectedFields -join ',')) { throw "$workflow CSV column mismatch" }
    $metadataPath = [IO.Path]::ChangeExtension($path, 'run.json')
    $metadata = Get-Content -LiteralPath $metadataPath -Raw | ConvertFrom-Json
    if ($metadata.simulation -ne $true) { throw "$workflow is not labelled simulated" }
    if ($metadata.status -ne 'complete' -or $metadata.rows -ne $rows.Count) { throw "$workflow metadata mismatch" }
    $rawPath = [IO.Path]::ChangeExtension($path, 'raw.csv')
    if ((Get-FileHash -LiteralPath $rawPath -Algorithm SHA256).Hash -ne $metadata.file_hashes.raw_csv_sha256) { throw "$workflow raw CSV hash mismatch" }
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $metadata.file_hashes.csv_sha256) { throw "$workflow main CSV hash mismatch" }
    foreach ($row in $rows) {
        if ($row.measurement -cne $workflow) { throw 'Wrong workflow in CSV' }
        $timestamp = [DateTimeOffset]::Parse($row.timestamp, [Globalization.CultureInfo]::InvariantCulture)
        if ($timestamp.Offset -ne [TimeSpan]::Zero) { throw 'Timestamp is not UTC' }
        foreach ($key in @('X','Y','R','Theta_deg')) {
            $value = [double]::Parse($row.$key, [Globalization.CultureInfo]::InvariantCulture)
            if ([double]::IsNaN($value) -or [double]::IsInfinity($value)) { throw "Non-finite $key" }
        }
    }
    $original = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    $retry = Start-Process -FilePath $binary -ArgumentList $arguments -WorkingDirectory $PackageDir -Wait -PassThru
    if ($retry.ExitCode -eq 0) { throw 'Existing output was accepted' }
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $original) { throw 'Existing output was modified' }
    $results += [pscustomobject]@{ workflow=$workflow; rows=$rows.Count; csv_sha256=$original; overwrite_refused=$true }
}

$report = [pscustomobject]@{
    host_name = $env:COMPUTERNAME
    windows_version = [Environment]::OSVersion.VersionString
    timestamp_utc = [DateTime]::UtcNow.ToString('o')
    executable_sha256 = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash
    package_hashes_verified = $true
    hardware_opened = $false
    results = $results
}
$report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $OutputDir 'validation.json') -Encoding UTF8
$report | ConvertTo-Json -Depth 5
