param(
    [Parameter(Mandatory = $true)]
    [string]$Source,

    [string]$DataHome = "",
    [string]$DbUriBase = "qnc+local://localhost/ingest-db/live",
    [string]$LanMountRoot = "",
    [string]$IntranetMountRoot = "",
    [string]$Ffprobe = "",
    [switch]$KeepBuild
)

$ErrorActionPreference = "Stop"

function Test-QncTransportUri {
    param([string]$Value)
    return $Value -match "^(qnc\+(local|lan|intranet)://|qnc://)"
}

$sourceText = $Source.Trim()
if ([string]::IsNullOrWhiteSpace($sourceText)) {
    throw "Source is required."
}

if (-not (Test-QncTransportUri $sourceText)) {
    $resolvedSource = Resolve-Path -LiteralPath $sourceText -ErrorAction Stop
    if (-not (Test-Path -LiteralPath $resolvedSource.ProviderPath -PathType Container)) {
        throw "Source must be a directory: $sourceText"
    }
    $sourceText = $resolvedSource.ProviderPath
}

if ($DbUriBase -notmatch "^qnc\+(local|lan|intranet)://") {
    throw "DbUriBase must be a QNC database URI base."
}

$scriptDir = Split-Path -Parent $PSCommandPath
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $scriptDir "..")).ProviderPath
$manifestPath = Join-Path $projectRoot "Cargo.toml"

if ([string]::IsNullOrWhiteSpace($DataHome)) {
    $localAppData = [Environment]::GetFolderPath("LocalApplicationData")
    if ([string]::IsNullOrWhiteSpace($localAppData)) {
        $localAppData = [System.IO.Path]::GetTempPath()
    }
    $DataHome = Join-Path $localAppData "IngestQNC\live"
}

$dataHomePath = [System.IO.Path]::GetFullPath($DataHome)
New-Item -ItemType Directory -Force -Path $dataHomePath | Out-Null

$targetDir = Join-Path ([System.IO.Path]::GetTempPath()) "ingestqnc-live-target"

Push-Location $projectRoot
try {
    cargo build --manifest-path $manifestPath --target-dir $targetDir --bin IngestQNC
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }

    $exePath = Join-Path $targetDir "debug\IngestQNC.exe"
    if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) {
        throw "Built executable not found: $exePath"
    }

    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = $exePath
    $psi.WorkingDirectory = $projectRoot
    $psi.UseShellExecute = $false
    $psi.Environment["INGESTQNC_HOME"] = $dataHomePath
    $psi.Environment["INGESTQNC_DB_URI_BASE"] = $DbUriBase.TrimEnd("/")
    $psi.Environment["INGESTQNC_INITIAL_SOURCE"] = $sourceText

    if (-not [string]::IsNullOrWhiteSpace($LanMountRoot)) {
        $psi.Environment["INGESTQNC_LAN_MOUNT_ROOT"] = [System.IO.Path]::GetFullPath($LanMountRoot)
    }
    if (-not [string]::IsNullOrWhiteSpace($IntranetMountRoot)) {
        $psi.Environment["INGESTQNC_INTRANET_MOUNT_ROOT"] = [System.IO.Path]::GetFullPath($IntranetMountRoot)
    }
    if (-not [string]::IsNullOrWhiteSpace($Ffprobe)) {
        $psi.Environment["INGESTQNC_FFPROBE"] = $Ffprobe
    }

    $process = [System.Diagnostics.Process]::Start($psi)
    Write-Host "IngestQNC live test started. PID: $($process.Id)"
    Write-Host "Source: $sourceText"
    Write-Host "Database home: $dataHomePath"
    Write-Host "Database URI base: $($psi.Environment["INGESTQNC_DB_URI_BASE"])"
    Write-Host "In the UI, click Odaberi, select clips, then click Ingest. Close IngestQNC when the live test is done."

    $process.WaitForExit()
    $exitCode = $process.ExitCode
}
finally {
    Pop-Location
}

if (-not $KeepBuild) {
    cargo clean --manifest-path $manifestPath --target-dir $targetDir
}

exit $exitCode
