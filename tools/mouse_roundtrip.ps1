param([string]$Binary='target/release/ajazz-lab.exe')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    $stamp=Get-Date -Format 'yyyyMMdd-HHmmss'
    $baselinePath=Join-Path $projectRoot "research/local/mouse-roundtrip-$stamp-before.json"
    $settingsPath=Join-Path $projectRoot "research/local/mouse-roundtrip-$stamp-settings.json"
    $beforeText=& $Binary mouse-snapshot $baselinePath
    if($LASTEXITCODE -ne 0){throw 'Mouse baseline read failed'}
    $before=($beforeText -join "`n") | ConvertFrom-Json
    $options=$before.options.PSObject.Copy()
    $options.debounce_ms=if($options.debounce_ms -eq 2){3}else{2}
    $options.polling_code=if($options.polling_code -eq 129){130}else{129}
    $options.lift_off=if($options.lift_off -eq 1){2}else{1}
    $options.angle_snap=if($options.angle_snap -eq 0){1}else{0}
    $dpi=$before.dpi.PSObject.Copy()
    $slot=if($dpi.current_index -eq 0){1}else{0}
    $dpi.x[$slot]=if($dpi.x[$slot] -eq 400){450}else{400}
    $dpi.y[$slot]=$dpi.x[$slot]
    $dpi.colors[$slot]=@(12,123,234)
    @{options=$options;dpi=$dpi} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $settingsPath -Encoding UTF8
    try {
        $receiptText=& $Binary mouse-apply $baselinePath $settingsPath
        if($LASTEXITCODE -ne 0){throw 'Mouse apply failed'}
        $receiptText | Set-Content "research/local/mouse-roundtrip-$stamp-receipt.json" -Encoding UTF8
        $receipt=($receiptText -join "`n") | ConvertFrom-Json
        [pscustomobject]@{phase='changed';verified=$receipt.changed;options=$receipt.snapshot.options;dpi=$receipt.snapshot.dpi.x[$slot];color=$receipt.snapshot.dpi.colors[$slot];backup=$receipt.backup_path} | ConvertTo-Json -Compress -Depth 4
    } finally {
        $restoreText=& $Binary mouse-restore $baselinePath
        if($LASTEXITCODE -ne 0){throw "Mouse restoration needs attention; baseline: $baselinePath"}
        $restoreText | Set-Content "research/local/mouse-roundtrip-$stamp-restored.json" -Encoding UTF8
        $restored=($restoreText -join "`n") | ConvertFrom-Json
        $equal=$true
        foreach($block in @('raw_options','raw_dpi')) {
            for($i=0;$i -lt 64;$i++) {
                if($i -ne 7 -and $before.$block[$i] -ne $restored.snapshot.$block[$i]){$equal=$false}
            }
        }
        [pscustomobject]@{phase='restored';all_settings_restored=$equal;baseline=$baselinePath} | ConvertTo-Json -Compress
        if(-not $equal){throw 'Restored mouse settings differ from baseline'}
    }
} finally {Pop-Location}
