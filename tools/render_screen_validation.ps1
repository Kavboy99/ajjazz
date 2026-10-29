# Offline Rust renderer fixtures used by the independent ARM emulator. No HID calls.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $root 'target/release/ajazz-lab.exe'
$output=Join-Path $root 'firmware/aj159-screen/build'
function Render([string[]]$arguments) {
    & $lab @arguments
    if($LASTEXITCODE -ne 0){throw "Renderer failed: $arguments"}
}
Render @('dock-screen-demo',"$output/telemetry-host.png")
foreach($row in @(@('summary',0,0),@('cpu',1,0),@('memory',2,0),@('clock',3,0),@('analog',5,0),@('calendar',6,0),@('focus',7,0),@('stopwatch',8,0),@('panel',0,1),@('minimal',0,2),@('modern-cpu',1,3),@('modern-memory',2,3),@('modern-clock',3,3))) {
    Render @('dock-page-demo',"$($row[1])","$output/page-$($row[0]).png","$($row[2])")
}
foreach($display in @('both','bars','percent')) {
    Render @('dock-game-demo',"$output/page-game-$display.png",$display)
    foreach($scenario in @('max','zero','missing','offline','fps')) {
        Render @('dock-game-demo',"$output/page-edge-$scenario-$display.png",$display,$scenario)
    }
    Render @('dock-game-demo',"$output/page-edge-paper-$display.png",$display,'max','paper')
}
