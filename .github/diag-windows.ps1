# TEMPORARY diagnostics for the Windows live failures (iter-304). Removed before merge.
$ErrorActionPreference = 'Continue'
$bin = Join-Path $PWD 'target\debug\ff-rdp.exe'
function Procs($label) {
  Write-Host "--- firefox processes: $label"
  Get-CimInstance Win32_Process -Filter "Name='firefox.exe'" | ForEach-Object {
    $cl = $_.CommandLine; if ($cl.Length -gt 160) { $cl = $cl.Substring(0,160) }
    Write-Host ("pid={0} ppid={1} {2}" -f $_.ProcessId, $_.ParentProcessId, $cl)
  }
}
function Listener($port) {
  Write-Host "--- netstat :$port"
  netstat -ano -p tcp | Select-String ":$port\s"
}
Write-Host "=== integrity"; whoami /groups | Select-String 'Mandatory'
Write-Host "=== launcher registry"; reg query 'HKCU\Software\Mozilla\Firefox\Launcher' 2>&1
Write-Host "=== process-start overhead"
1..3 | ForEach-Object { $t = Measure-Command { & $bin --version | Out-Null }; Write-Host ("--version {0:N0} ms" -f $t.TotalMilliseconds) }

$env:FF_RDP_HOME = Join-Path $env:RUNNER_TEMP 'diag-home'
New-Item -ItemType Directory -Force $env:FF_RDP_HOME | Out-Null
Procs 'before'

Write-Host "=== launch 6111"
& $bin launch --headless --debug-port 6111
Procs 'after launch'; Listener 6111
foreach ($cmd in @('tabs')) {
  $t = Measure-Command { & $bin --host 127.0.0.1 --port 6111 $cmd | Out-Null }
  Write-Host ("$cmd {0:N0} ms" -f $t.TotalMilliseconds)
}
Write-Host "=== oneway navigate trace"
$env:NO_COLOR = '1'
$t = Measure-Command {
  & $bin --host 127.0.0.1 --port 6111 --log-level trace navigate 'data:text/html,<h1>oneway-test</h1>' --timeout 8000 --allow-unsafe-urls --with-network --network-timeout 1000 > "$env:RUNNER_TEMP\nav.out" 2> "$env:RUNNER_TEMP\nav.err"
}
Write-Host ("navigate wall {0:N0} ms" -f $t.TotalMilliseconds)
Get-Content "$env:RUNNER_TEMP\nav.err" | ForEach-Object {
  if ($_ -match '^(\S+).*direction="?(\w+)"?\s+actor=(\S+)\s+kind=(\S+)') { "{0} {1} {2} {3}" -f $Matches[1], $Matches[2], $Matches[4], $Matches[3] } else { $_.Substring(0, [Math]::Min(200, $_.Length)) }
}
Write-Host "=== back trace (no history)"
$t = Measure-Command {
  & $bin --host 127.0.0.1 --port 6111 --log-level debug back --timeout 3000 > "$env:RUNNER_TEMP\back.out" 2> "$env:RUNNER_TEMP\back.err"
}
Write-Host ("back wall {0:N0} ms" -f $t.TotalMilliseconds)
Get-Content "$env:RUNNER_TEMP\back.out"
Get-Content "$env:RUNNER_TEMP\back.err" | Select-Object -First 40 | ForEach-Object { $_.Substring(0, [Math]::Min(220, $_.Length)) }
& $bin stop --port 6111
Procs 'after stop'

Write-Host "=== bad-manifest launch 6112"
& $bin launch --headless --debug-port 6112 --temporary-addon (Join-Path $PWD 'crates\ff-rdp-cli\tests\fixtures\temporary_addon_bad_manifest')
Procs 'right after failed launch'; Listener 6112
Start-Sleep -Seconds 3
Procs '3s after failed launch'; Listener 6112
Start-Sleep -Seconds 10
Procs '13s after failed launch'; Listener 6112
Get-ChildItem -Recurse (Join-Path $env:FF_RDP_HOME 'ff-rdp\profiles') -ErrorAction SilentlyContinue | Select-Object -First 5 FullName

Write-Host "=== raw firefox 6113"
$prof = Join-Path $env:RUNNER_TEMP 'raw-diag-profile'
New-Item -ItemType Directory -Force $prof | Out-Null
Set-Content (Join-Path $prof 'user.js') "user_pref(`"devtools.debugger.remote-enabled`", true);`nuser_pref(`"devtools.chrome.enabled`", true);`nuser_pref(`"devtools.debugger.prompt-connection`", false);`n"
$p = Start-Process -PassThru 'C:\Program Files\Mozilla Firefox\firefox.exe' -ArgumentList @('-no-remote','-headless','-profile',$prof,'--start-debugger-server','6113')
Write-Host "spawned pid $($p.Id)"
Start-Sleep -Seconds 12
Procs 'raw firefox'; Listener 6113
& $bin stop --port 6113
Get-CimInstance Win32_Process -Filter "Name='firefox.exe'" | Where-Object { $_.CommandLine -like '*raw-diag-profile*' -or $_.CommandLine -like '*diag-home*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
exit 0
