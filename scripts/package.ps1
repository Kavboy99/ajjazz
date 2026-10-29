param([string]$OutputDirectory='dist')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$output=Join-Path $projectRoot $OutputDirectory
$artifacts=Join-Path $projectRoot 'artifacts'
New-Item -ItemType Directory -Force -Path $output,$artifacts | Out-Null
foreach($name in @('ajazz-studio.exe','ajazz-lab.exe')) {
    $binary=Join-Path $projectRoot "target\release\$name"
    if(-not(Test-Path -LiteralPath $binary)){throw "Build release first: $binary"}
    $destination=Join-Path $output $name
    if((Test-Path -LiteralPath $destination) -and ((Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash -eq (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash)){continue}
    Copy-Item -LiteralPath $binary -Destination $output -Force
}
foreach($name in @('README.md','LICENSE')){Copy-Item -LiteralPath (Join-Path $projectRoot $name) -Destination $output -Force}
foreach($name in @('docs','examples')){Copy-Item -LiteralPath (Join-Path $projectRoot $name) -Destination $output -Recurse -Force}
$continuationCopy=Join-Path $output 'docs\CONTINUATION-AJAZZ.md'
if(Test-Path -LiteralPath $continuationCopy){Remove-Item -LiteralPath $continuationCopy}
$presentMonSource=Join-Path $projectRoot 'research/vendor/presentmon-2.6.0'
$presentMonOutput=Join-Path $output 'tools/presentmon'
if((Get-FileHash -LiteralPath (Join-Path $presentMonSource 'PresentMon.exe') -Algorithm SHA256).Hash.ToLower() -ne 'b2a706bc6ad475749e3b7e3409263aa1e6906d45bdcf993f6dbc0f660188f1af'){throw 'Unverified PresentMon component'}
New-Item -ItemType Directory -Force -Path $presentMonOutput | Out-Null
Copy-Item -LiteralPath (Join-Path $presentMonSource 'PresentMon.exe'),(Join-Path $presentMonSource 'LICENSE.txt') -Destination $presentMonOutput -Force
$sensorsSource=Join-Path $projectRoot 'research/vendor/ajazz-sensors'
$sensorsOutput=Join-Path $output 'tools/sensors'
$sensorsManifest=Get-Content -LiteralPath (Join-Path $projectRoot 'assets/sensors-manifest.json') -Raw | ConvertFrom-Json -AsHashtable
New-Item -ItemType Directory -Force -Path $sensorsOutput | Out-Null
foreach($name in $sensorsManifest.Keys){
    $component=Join-Path $sensorsSource $name
    if((Get-FileHash -LiteralPath $component -Algorithm SHA256).Hash.ToLower() -ne $sensorsManifest[$name]){throw "Unverified sensor component: $name"}
    Copy-Item -LiteralPath $component -Destination $sensorsOutput -Force
}
Copy-Item -LiteralPath (Join-Path $projectRoot 'assets/sensors-manifest.json') -Destination $sensorsOutput -Force
Copy-Item -LiteralPath (Join-Path $projectRoot 'tools/sensors/AjazzSensors.cs') -Destination $sensorsOutput -Force
$updatesSource=Join-Path $projectRoot 'research/vendor/studio-updates'
$updatesOutput=Join-Path $output 'tools/updates'
$updateManifest=Get-Content -LiteralPath (Join-Path $projectRoot 'assets/update-manifest.json') -Raw | ConvertFrom-Json
foreach($entry in $updateManifest.entries){
    $targetOutput=Join-Path $updatesOutput $entry.target
    foreach($property in $entry.files.PSObject.Properties){
        $source=Join-Path (Join-Path $updatesSource $entry.target) $property.Name
        if((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLower() -ne $property.Value){throw "Unverified updater file: $source"}
        $destination=Join-Path $targetOutput $property.Name
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $destination) | Out-Null
        Copy-Item -LiteralPath $source -Destination $destination -Force
    }
}
$sensorInstaller=Join-Path $updatesSource 'PawnIO_setup.exe'
if((Get-FileHash -LiteralPath $sensorInstaller -Algorithm SHA256).Hash.ToLower() -ne $updateManifest.sensor_installer_sha256){throw 'Unverified sensor installer'}
Copy-Item -LiteralPath $sensorInstaller -Destination $updatesOutput -Force
Copy-Item -LiteralPath (Join-Path $projectRoot 'assets/update-manifest.json') -Destination $updatesOutput -Force
$hashes=Get-ChildItem -LiteralPath $output -Filter '*.exe' | ForEach-Object {"$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower())  $($_.Name)"}
[IO.File]::WriteAllLines((Join-Path $output 'SHA256SUMS.txt'),$hashes)
$version=(Select-String -LiteralPath (Join-Path $projectRoot 'Cargo.toml') -Pattern '^version = "([^"]+)"').Matches.Groups[1].Value
$zip=Join-Path $artifacts "AJAZZ-Studio-$version-windows-x64.zip"
Compress-Archive -Path (Join-Path $output '*') -DestinationPath $zip -Force
Get-Item -LiteralPath $zip | Select-Object FullName,Length
