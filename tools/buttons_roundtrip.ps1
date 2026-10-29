param([string]$Binary='target/debug/ajazz-lab.exe',[switch]$KeyChord)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    $stamp=Get-Date -Format 'yyyyMMdd-HHmmss'
    $baseline="research/local/buttons-$stamp-before.json"
    $text=& $Binary mouse-buttons $baseline
    if($LASTEXITCODE -ne 0){throw 'Button baseline read failed; no writes started'}
    $before=($text -join "`n") | ConvertFrom-Json
    $performance=& $Binary mouse-snapshot "research/local/buttons-$stamp-performance-before.json"
    if($LASTEXITCODE -ne 0){throw 'Performance baseline read failed; no writes started'}
    $performanceBefore=($performance -join "`n") | ConvertFrom-Json
    # Change only the back button; primary clicks remain available.
    $action=if($before.raw_matrix[18] -eq 243){'Forward'}else{'Back'}
    $edit="research/local/buttons-$stamp-edit.json"
    $assignment=if($KeyChord){@{Key=@{usage=6;modifier='Ctrl'}}}else{$action}
    @{slot=4;action=$assignment} | ConvertTo-Json -Depth 5 | Set-Content $edit -Encoding UTF8
    try {
        $result=& $Binary mouse-buttons-apply $baseline $edit
        if($LASTEXITCODE -ne 0){throw 'Button apply failed'}
        $result | Set-Content "research/local/buttons-$stamp-receipt.json" -Encoding UTF8
        $receipt=($result -join "`n") | ConvertFrom-Json
        [pscustomobject]@{phase='changed';verified=$receipt.changed;slot4=$receipt.snapshot.raw_matrix[16..19];backup=$receipt.backup_path} | ConvertTo-Json -Compress
    } finally {
        $result=& $Binary mouse-buttons-restore $baseline
        if($LASTEXITCODE -ne 0){throw "Button restoration needs attention: $baseline"}
        $result | Set-Content "research/local/buttons-$stamp-restored.json" -Encoding UTF8
        $restored=($result -join "`n") | ConvertFrom-Json
        $equal=($before.raw_matrix -join ',') -eq ($restored.snapshot.raw_matrix -join ',')
        [pscustomobject]@{phase='restored';all_64_bytes_equal=$equal;baseline=$baseline} | ConvertTo-Json -Compress
        if(-not $equal){throw 'Mouse button matrix differs from baseline'}
        $performance=& $Binary mouse-snapshot "research/local/buttons-$stamp-performance-after.json"
        if($LASTEXITCODE -ne 0){throw 'Performance verification read failed'}
        $performanceAfter=($performance -join "`n") | ConvertFrom-Json
        $equal=$true
        foreach($block in @('raw_options','raw_dpi')) {
            for($i=0;$i -lt 64;$i++) {
                if($i -ne 7 -and $performanceBefore.$block[$i] -ne $performanceAfter.$block[$i]){$equal=$false}
            }
        }
        [pscustomobject]@{phase='performance';all_d3_d4_settings_equal=$equal} | ConvertTo-Json -Compress
        if(-not $equal){throw 'Mouse performance changed during button assignment test'}
    }
} finally {Pop-Location}
