<#
.SYNOPSIS
Build full/selective/default firmware under identical conditions; never flash.
#>
param(
    [string]$Python = 'C:\Users\James\AppData\Local\Python\pythoncore-3.14-64\python.exe',
    [string]$OutputDirectory = ('build/logger-firmware-study-' + [guid]::NewGuid().ToString('N')),
    [string]$TargetDirectory = 'target/logger-selective-arm-study'
)
$ErrorActionPreference = 'Stop'
$studyRoot = Split-Path -Parent $PSScriptRoot
Push-Location $studyRoot
$studyOldPath = $env:PATH
$studyOldCargoHome = $env:CARGO_HOME
try {
    $env:PATH = (Join-Path $studyRoot 'target/capture-toolchain/bin-dist/bin') + ';' + $env:PATH
    $env:CARGO_HOME = Join-Path $studyRoot 'target/capture-toolchain/cargo'
    # Fail instead of overwriting any prior ELF/log/evidence directory.
    if (Test-Path -LiteralPath $OutputDirectory) { throw 'Output directory already exists' }
    New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
    $studyOutput = (Resolve-Path -LiteralPath $OutputDirectory).Path
    $studySources = @{}
    $studyFiles = @(Get-ChildItem firmware,firmware-core,firmware-image,fusb302-pd -Recurse -File)
    $studyFiles += @(Get-Item Cargo.toml,Cargo.lock,.cargo/config.toml)
    foreach ($studyFile in $studyFiles) {
        $studyRelative = [IO.Path]::GetRelativePath($studyRoot, $studyFile.FullName).Replace('\', '/')
        $studySources[$studyRelative] = (Get-FileHash -LiteralPath $studyFile.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $studyManifest = [ordered]@{
        base_head = (git rev-parse HEAD).Trim()
        cargo = (cargo --version).Trim()
        rustc = @(rustc --version --verbose)
        profile = 'release: opt-level=z, lto=true, codegen-units=1'
        target = 'thumbv7em-none-eabihf'
        rustflags = $env:RUSTFLAGS
        encoded_rustflags = $env:CARGO_ENCODED_RUSTFLAGS
        hardware_access = $false
        sources_sha256 = $studySources
        builds = @()
    }
    foreach ($studyBuild in @(
        @{label='full-logger'; features='charging-logger'},
        @{label='selective-logger'; features='selective-logger'},
        @{label='default'; features=''}
    )) {
        $studyArguments = @('build','-p','firmware','--bin','firmware','--release',
            '--target','thumbv7em-none-eabihf','--target-dir',$TargetDirectory,'--locked','--offline')
        if ($studyBuild.features) { $studyArguments += @('--features', $studyBuild.features) }
        Write-Output "Building $($studyBuild.label)"
        $studyLog = Join-Path $studyOutput ($studyBuild.label + '.log')
        & cargo @studyArguments *> $studyLog
        if ($LASTEXITCODE -ne 0) {
            Get-Content -LiteralPath $studyLog -Tail 45
            throw "Build failed: $($studyBuild.label)"
        }
        $studyElf = Join-Path $studyOutput ($studyBuild.label + '.elf')
        Copy-Item -LiteralPath (Join-Path $TargetDirectory 'thumbv7em-none-eabihf/release/firmware') -Destination $studyElf
        $studyManifest.builds += @{
            label = $studyBuild.label
            requested_features = $studyBuild.features
            command_arguments = $studyArguments
            elf_sha256 = (Get-FileHash -LiteralPath $studyElf -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    }
    # Detect source changes during the sequence rather than comparing different inputs.
    foreach ($studyRelative in $studySources.Keys) {
        $studyHash = (Get-FileHash -LiteralPath (Join-Path $studyRoot $studyRelative) -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($studyHash -ne $studySources[$studyRelative]) { throw "Source changed: $studyRelative" }
    }
    $studyManifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $studyOutput 'manifest.json') -Encoding utf8
    & $Python tools/logger_firmware_footprint.py --elf "full-logger=$(Join-Path $studyOutput 'full-logger.elf')" --elf "selective-logger=$(Join-Path $studyOutput 'selective-logger.elf')" --elf "default=$(Join-Path $studyOutput 'default.elf')" --out (Join-Path $studyOutput 'footprint.json')
    if ($LASTEXITCODE -ne 0) { throw 'ELF inspection failed' }
    Write-Output "Saved comparison: $studyOutput"
} finally {
    $env:PATH = $studyOldPath
    $env:CARGO_HOME = $studyOldCargoHome
    Pop-Location
}
