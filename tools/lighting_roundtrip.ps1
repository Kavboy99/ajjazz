param([string]$Binary='target/debug/ajazz-lab.exe')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $projectRoot $Binary
$local=Join-Path $projectRoot 'research/local'
function Invoke-Lab([string[]]$Arguments) {
    $result=& $lab @Arguments
    if($LASTEXITCODE -ne 0){throw "ajazz-lab failed: $Arguments"}
    return $result
}
$beforePath=Join-Path $local 'lighting-before-test.json'
$duringPath=Join-Path $local 'lighting-during-test.json'
$afterPath=Join-Path $local 'lighting-after-test.json'
Invoke-Lab @('snapshot',$beforePath) | Out-Null
$receipt=$null
try {
    $receipt=(Invoke-Lab @('lighting-apply',(Join-Path $projectRoot 'examples/aurora.ajazz.json'),'0')) -join "`n" | ConvertFrom-Json
    Invoke-Lab @('snapshot',$duringPath) | Out-Null
} finally {
    if($null -ne $receipt) {
        Invoke-Lab @('lighting-restore',$receipt.backup_path) | Set-Content -LiteralPath (Join-Path $local 'lighting-restore-receipt.json') -Encoding UTF8
    }
}
Invoke-Lab @('snapshot',$afterPath) | Out-Null
$before=Get-Content -Raw -LiteralPath $beforePath | ConvertFrom-Json
$during=Get-Content -Raw -LiteralPath $duringPath | ConvertFrom-Json
$after=Get-Content -Raw -LiteralPath $afterPath | ConvertFrom-Json
function Equal-Bytes($Left,$Right) { return ($Left -join ',') -ceq ($Right -join ',') }
$otherBlocksUnchanged=$true
foreach($name in @('keymap','game_mode','rapid_trigger','dks')) {
    if(-not(Equal-Bytes $before.blocks.$name $after.blocks.$name)){$otherBlocksUnchanged=$false}
}
$lightingRestored=$true
foreach($index in (0..15 | Where-Object {$_ -notin @(4,14,15)})) {
    if($before.blocks.lighting[$index] -ne $after.blocks.lighting[$index]){$lightingRestored=$false}
}
$proof=[ordered]@{
    timestamp_utc=[DateTime]::UtcNow.ToString('o')
    version=$before.info.version
    backup_path=$receipt.backup_path
    rgb_changed=(-not(Equal-Bytes $before.blocks.custom_rgb $during.blocks.custom_rgb))
    during_mode=$during.blocks.lighting[0]
    rgb_restored=(Equal-Bytes $before.blocks.custom_rgb $after.blocks.custom_rgb)
    lighting_restored=$lightingRestored
    other_blocks_unchanged=$otherBlocksUnchanged
    physical_led_appearance_observed=$false
}
$proof | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $local 'lighting-roundtrip.json') -Encoding UTF8
$proof | ConvertTo-Json
if(-not($proof.rgb_changed -and $proof.rgb_restored -and $proof.lighting_restored -and $proof.other_blocks_unchanged)) { throw 'Hardware round-trip did not pass' }
