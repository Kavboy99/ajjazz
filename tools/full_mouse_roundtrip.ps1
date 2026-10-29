param([string]$Binary='target/debug/ajazz-lab.exe')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    $stamp=Get-Date -Format 'yyyyMMdd-HHmmss'
    $baseline="research/local/mouse-full-$stamp-before.json"
    $text=& $Binary mouse-snapshot $baseline
    if($LASTEXITCODE -ne 0){throw 'Mouse baseline unavailable; no writes started'}
    $before=($text -join "`n") | ConvertFrom-Json
    $settings="research/local/mouse-full-$stamp-settings.json"
    @{options=$before.options;dpi=$before.dpi;sleep=@{radio_seconds=300}} | ConvertTo-Json -Depth 8 | Set-Content $settings -Encoding UTF8
    try {
        $text=& $Binary mouse-apply $baseline $settings
        if($LASTEXITCODE -ne 0){throw 'Temporary sleep setting failed'}
        $text | Set-Content "research/local/mouse-full-$stamp-extended-sleep.json" -Encoding UTF8
        $extended=($text -join "`n") | ConvertFrom-Json
        $seconds=[int]$extended.snapshot.raw_options[44]+256*[int]$extended.snapshot.raw_options[45]
        if($seconds -ne 300){throw 'Sleep setting not verified'}
        Write-Output 'temporary_sleep_seconds=300 verified=true'
        & "$PSScriptRoot/buttons_roundtrip.ps1" -Binary $Binary -KeyChord
        & "$PSScriptRoot/mouse_roundtrip.ps1" -Binary $Binary
    } finally {
        $text=& $Binary mouse-restore $baseline
        if($LASTEXITCODE -ne 0){throw "Mouse restoration needs attention: $baseline"}
        $text | Set-Content "research/local/mouse-full-$stamp-restored.json" -Encoding UTF8
        $restored=($text -join "`n") | ConvertFrom-Json
        $equal=$true
        foreach($block in @('raw_options','raw_dpi')) {
            for($i=0;$i -lt 64;$i++) {
                if($i -ne 7 -and $before.$block[$i] -ne $restored.snapshot.$block[$i]){$equal=$false}
            }
        }
        $seconds=[int]$restored.snapshot.raw_options[44]+256*[int]$restored.snapshot.raw_options[45]
        [pscustomobject]@{phase='final';all_mouse_settings_restored=$equal;sleep_seconds=$seconds;baseline=$baseline} | ConvertTo-Json -Compress
        if(-not $equal){throw 'Restored mouse state differs from baseline'}
    }
} finally {Pop-Location}
