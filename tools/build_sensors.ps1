$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$vendor=Join-Path $projectRoot 'research/vendor/librehardwaremonitor-0.9.6/unpacked'
$output=Join-Path $projectRoot 'research/vendor/ajazz-sensors'
New-Item -ItemType Directory -Force -Path $output | Out-Null
# Dependencies are the unmodified official v0.9.6 release. UI assemblies aren't used.
Get-ChildItem -LiteralPath $vendor -Filter '*.dll' | Where-Object {$_.Name -notmatch '^(Aga|OxyPlot|Microsoft.Win32.TaskScheduler)'} | Copy-Item -Destination $output -Force
$compiler=Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
& $compiler /nologo /target:exe /platform:x64 /optimize+ "/out:$output/AjazzSensors.exe" "/reference:$output/LibreHardwareMonitorLib.dll" /reference:System.Web.Extensions.dll (Join-Path $projectRoot 'tools/sensors/AjazzSensors.cs')
if($LASTEXITCODE -ne 0){throw 'Sensor bridge compilation failed'}
$manifest=@{}
Get-ChildItem -LiteralPath $output -File | ForEach-Object {$manifest[$_.Name]=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower()}
[IO.File]::WriteAllText((Join-Path $projectRoot 'assets/sensors-manifest.json'),($manifest|ConvertTo-Json))
$debugOutput=Join-Path $projectRoot 'target/debug/tools/sensors'
New-Item -ItemType Directory -Force -Path $debugOutput | Out-Null
Copy-Item -Path (Join-Path $output '*') -Destination $debugOutput -Force
Write-Output "Built read-only sensor bridge with $($manifest.Count) pinned files."
