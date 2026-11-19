---
title: "Windows launch on lenny: baseline and lab notes (2026-11-18)"
type: research
date: 2026-11-18
tags: [research, windows, launch]
---

# Windows launch on lenny: baseline and lab notes

Hand-over notes for [[iteration-302-windows-listtabs-launch-failure]]. Everything here was run
from the Mac through Herdr (`hc exec --on lenny …`) on 2026-11-18.

## The machine

- **lenny**: Windows 11 Pro 10.0.26200, **German UI**, AMD64, 12 cores, 64 GB RAM, Firefox **160.0**
  in `C:\Program Files\Mozilla Firefox\`.
  - Tooling: Rust 1.100 (MSVC), VS 2022 Build Tools (VCTools), pwsh 7.7 (app alias in
    `%LOCALAPPDATA%\Microsoft\WindowsApps`), Git for Windows.
  - Shells: the SSH/Herdr session is **elevated** (admin token), like the GitHub runner. The
    default SSH shell is Windows PowerShell 5.1, and `hc exec` runs through `cmd.exe`.
- **The owner's desktop Firefox runs on lenny** (about 20 `firefox.exe` from the same install).
  - **Never** `taskkill /IM firefox.exe` or `Stop-Process -Name firefox`. That rules out the
    `Kill-All` / `No-Firefox-Left` helpers of `.github/workflows/windows-launch.yml`.
  - Tear down only your own instance: by the PID `launch` printed, or by matching
    `--start-debugger-server <port>` in the command line (the scripts below do this).
  - Firefox content processes don't carry that argument, but they exit with their parent:
    0 orphans after 80 launches.
- Checkouts:
  - `C:\Users\james\devel\ff-rdp`: the owner's clone. Work here.
  - `C:\Users\james\ffrdp-work\`: my lab, holding the scripts, `v050\ff-rdp.exe` (v0.5.0 from the
    release zip), `bin-main\ff-rdp.exe` (main at d11ef932, before #387), `runs\<label>\` (per-run
    `summary.jsonl`, failed launch stderr with `--log-level debug`) and a clone `ff-rdp\`. Safe to
    delete when done.

## Baseline: launch without a URL, 20 runs each (2026-11-18)

Each run: `--log-level debug launch [--headless] --temp-profile --port P` with a fresh
`FF_RDP_HOME`, then `--port P stop`, then a by-port safety net.

| Binary | Mode | Failed | Median ok | Max ok | Failure text |
|---|---|---|---|---|---|
| v0.5.0 | headless | 1/20 | 2.2 s | 3.1 s | `did not open debug port P within 30s` |
| main d11ef932 | headless | **0/20** | 4.6 s | 10.6 s | — |
| v0.5.0 | headed | 20/20 | — | — | `Firefox exited immediately with exit code: 0` (the launcher exits at once, fixed by #383) |
| main d11ef932 | headed | **0/20** | 5.1 s | 6.2 s | — |

- **The CI failure did not reproduce on lenny in 80 launches.** The failure: `opened debug port
  … but listed no tab within 30s/60s (last listTabs error: … browsingContext.currentWindowGlobal
  is null)`.
- Every `stop` with `bin-main` was refused, and every launch took ~2 s longer than v0.5.0. Both
  are the locale bug fixed in #387: the launcher hand-over retried for ~2 s and gave up.
  *Unverified*: after #387, headless launch should be back near v0.5.0's 2.2 s. Re-measure.

## CI evidence (GitHub windows-latest, Firefox 159.0.1, image 20261103)

`windows-launch` attempts that failed with the listTabs error:
- run 44169619613: A2 in attempt 1, B in attempt 2 (head 85eb72e, #383)
- run 44184491201 attempt 1 (head a66fe62, #385, 30 s wait)
- run 44186427298 attempts 1 and 2: scenario C, headed, 60 s wait (head e1c542f, #385)
- run 44335920791: A2 (head e5bd19ff, #387)

All are launches without a URL, so none reach iter-300's startup-URL wait. The error persists for
the full 60 s, which looks like a tab that never gets a window global, not a slow one.

Differences between the runner and lenny:
- Firefox 159.0.1 vs 160.0;
- a 4-vCPU VM vs 12 cores;
- English vs German UI;
- the runner force-kills every `firefox.exe` between scenarios, while lenny stops by port.

## Scripts

`baseline.ps1`: one loop of N launches with one binary.

```powershell
# Baseline: N launches without a URL with one ff-rdp binary, each stopped by its
# own port. Teardown only touches firefox.exe whose command line names this run's
# --start-debugger-server port: the owner's desktop Firefox runs on this machine.
param(
  [Parameter(Mandatory)][string]$Exe,
  [Parameter(Mandatory)][string]$Label,
  [int]$N = 20,
  [int]$BasePort = 6200,
  [switch]$Headed
)
$ErrorActionPreference = 'Continue'
$W = "$env:USERPROFILE\ffrdp-work\runs\$Label"
New-Item -ItemType Directory -Force $W | Out-Null
$summary = Join-Path $W 'summary.jsonl'
Remove-Item $summary -ErrorAction SilentlyContinue

function Ours([int]$port) {
  @(Get-CimInstance Win32_Process -Filter "Name='firefox.exe'" |
    Where-Object { $_.CommandLine -match "--start-debugger-server\s+$port(\s|$)" })
}

for ($i = 1; $i -le $N; $i++) {
  $port = $BasePort + $i
  $home_ = Join-Path $W "home-$i"
  New-Item -ItemType Directory -Force $home_ | Out-Null
  $o = Join-Path $W "launch-$i.out"; $e = Join-Path $W "launch-$i.err"
  $a = @('--log-level', 'debug', 'launch', '--temp-profile', '--port', "$port")
  if ($Headed) { $a += @('--launch-timeout', '60') } else { $a = @('--log-level', 'debug', 'launch', '--headless', '--temp-profile', '--port', "$port") }
  $env:FF_RDP_HOME = $home_
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $p = Start-Process -FilePath $Exe -ArgumentList $a -RedirectStandardOutput $o -RedirectStandardError $e -NoNewWindow -PassThru
  $null = $p.Handle
  $hung = -not $p.WaitForExit(150000)
  if ($hung) { $p.Kill() }
  $p.WaitForExit()
  $ms = $sw.ElapsedMilliseconds
  $raw = Get-Content $o -Raw -ErrorAction SilentlyContinue
  $j = $null; try { if ($raw) { $j = $raw | ConvertFrom-Json } } catch {}
  $err = if ($j -and $j.error) { "$($j.error)" } elseif (-not $raw) { (Get-Content $e -Tail 1 -ErrorAction SilentlyContinue) } else { '' }
  $procsAtLaunch = (Ours $port).Count

  # stop by port, then the safety net by command line
  $so = Join-Path $W "stop-$i.out"
  $sp = Start-Process -FilePath $Exe -ArgumentList @('--port', "$port", 'stop') -RedirectStandardOutput $so -RedirectStandardError "$so.err" -NoNewWindow -PassThru
  $null = $sp.Handle
  if (-not $sp.WaitForExit(60000)) { $sp.Kill() }
  $deadline = (Get-Date).AddSeconds(15)
  while ((Ours $port).Count -gt 0 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250 }
  $leaked = (Ours $port).Count
  foreach ($q in (Ours $port)) { Stop-Process -Id $q.ProcessId -Force -ErrorAction SilentlyContinue }

  $ok = ($p.ExitCode -eq 0 -and -not $hung)
  $rec = [ordered]@{ i = $i; label = $Label; port = $port; ok = $ok; exit = $p.ExitCode; hung = $hung; ms = $ms;
    pid = $j.results.pid; launcher_pid = $j.results.launcher_pid; ff_procs = $procsAtLaunch; leaked_after_stop = $leaked;
    error = $err }
  ($rec | ConvertTo-Json -Compress) | Add-Content -Encoding utf8 $summary
  "{0,3} ok={1} exit={2} ms={3} leaked={4} {5}" -f $i, $ok, $p.ExitCode, $ms, $leaked, $err
  if ($ok) { Remove-Item $e -ErrorAction SilentlyContinue }   # keep debug logs of failures only
  Remove-Item -Recurse -Force $home_ -ErrorAction SilentlyContinue
}
$all = Get-Content $summary | ConvertFrom-Json
"RESULT $Label failed=$(@($all | Where-Object { -not $_.ok }).Count)/$N leaked_runs=$(@($all | Where-Object { $_.leaked_after_stop -gt 0 }).Count)"
"BASELINE-DONE"
```

`baseline-all.ps1`: the four loops above, with the main binary copied first so a rebuild can't
replace it mid-run.

```powershell
$W = "$env:USERPROFILE\ffrdp-work"
New-Item -ItemType Directory -Force "$W\bin-main" | Out-Null
Copy-Item -Force "$W\ff-rdp\target\debug\ff-rdp.exe" "$W\bin-main\ff-rdp.exe"
function Orphans { @(Get-CimInstance Win32_Process -Filter "Name='firefox.exe'" | Where-Object { $_.CommandLine -like '*-contentproc*' -and -not (Get-Process -Id $_.ParentProcessId -ErrorAction SilentlyContinue) }).Count }
"firefox before: " + @(Get-Process firefox).Count + " orphans: " + (Orphans)
& pwsh -NoProfile -File "$W\baseline.ps1" -Exe "$W\v050\ff-rdp.exe" -Label v050-headless -N 20 -BasePort 6400
& pwsh -NoProfile -File "$W\baseline.ps1" -Exe "$W\bin-main\ff-rdp.exe" -Label main-headless -N 20 -BasePort 6500
& pwsh -NoProfile -File "$W\baseline.ps1" -Exe "$W\v050\ff-rdp.exe" -Label v050-headed -N 20 -BasePort 6600 -Headed
& pwsh -NoProfile -File "$W\baseline.ps1" -Exe "$W\bin-main\ff-rdp.exe" -Label main-headed -N 20 -BasePort 6700 -Headed
"firefox after: " + @(Get-Process firefox).Count + " orphans: " + (Orphans)
"ALL-BASELINES-DONE"
```

`stats.ps1`: the table above, from the `summary.jsonl` files.

```powershell
foreach ($l in 'v050-headless','main-headless','v050-headed','main-headed') {
  $r = Get-Content "$env:USERPROFILE\ffrdp-work\runs\$l\summary.jsonl" | ConvertFrom-Json
  $ok = @($r | Where-Object ok); $ms = @($ok | ForEach-Object ms | Sort-Object)
  $med = if ($ms.Count) { $ms[[int][math]::Floor($ms.Count/2)] } else { '-' }
  $errs = ($r | Where-Object { -not $_.ok } | Group-Object { ($_.error -replace 'pid \d+','pid N' -replace 'port \d+','port P') } | ForEach-Object { "$($_.Count)x $($_.Name)" }) -join ' | '
  "{0}: failed {1}/{2}; median ok ms {3}; max ok ms {4}; leaked-after-stop runs {5}; {6}" -f $l, ($r.Count-$ok.Count), $r.Count, $med, ($ms | Select-Object -Last 1), @($r | Where-Object { $_.leaked_after_stop -gt 0 }).Count, $errs
}
```

`listen.ps1`: one launch, then the port's listener, its `netstat` row and the process tree.

```powershell
$env:FF_RDP_HOME = "$env:USERPROFILE\ffrdp-work\runs\listen"; New-Item -ItemType Directory -Force $env:FF_RDP_HOME | Out-Null
$exe = "$env:USERPROFILE\ffrdp-work\ff-rdp\target\debug\ff-rdp.exe"
$j = & $exe launch --headless --temp-profile --port 6399 | ConvertFrom-Json
"launch pid=$($j.results.pid) launcher=$($j.results.launcher_pid)"
Get-NetTCPConnection -LocalPort 6399 -State Listen | Select-Object LocalAddress,LocalPort,OwningProcess | Format-Table -AutoSize | Out-String
netstat -ano | Select-String ':6399 ' | Select-Object -First 4 | ForEach-Object { $_.Line }
Get-CimInstance Win32_Process -Filter "Name='firefox.exe'" | Where-Object { $_.CommandLine -match '--start-debugger-server\s+6399' } | Select-Object ProcessId,ParentProcessId | Format-Table -AutoSize | Out-String
foreach ($q in (Get-CimInstance Win32_Process -Filter "Name='firefox.exe'" | Where-Object { $_.CommandLine -match '--start-debugger-server\s+6399' })) { Stop-Process -Id $q.ProcessId -Force -ErrorAction SilentlyContinue }
Start-Sleep 2; Remove-Item -Recurse -Force $env:FF_RDP_HOME -ErrorAction SilentlyContinue
"desktop firefox procs: " + @(Get-Process firefox).Count
```

## 2026-11-19: the failure reproduces on lenny — Firefox cannot start its content processes

Firefox **160.0.1** (it self-updated from 160.0 on 2026-11-18 20:51), **non-elevated** shell,
binary `bin-302\ff-rdp.exe` (branch head adb6a392). Runs under `C:\Users\james\ffrdp-work\runs\`.

- `302-nonelev-headless-raw` (1/15 failed) and `302-nonelev-headless-raw60` (2/60 failed):
  `baseline.ps1` with `FF_RDP_TRACE_RAW=1`. Failure text, identical to CI: `opened debug port …
  but listed no tab within 30s (last listTabs error: … browsingContext.currentWindowGlobal is
  null)`. The kept `launch-14.err`, `launch-3.err`, `launch-6.err` hold Firefox's raw stderr.
- In every failing launch Firefox's sandbox launcher fails to start child processes from ~0.8 s
  after the start and for the whole wait, 17–20 times:
  `[Parent 25704, IPC I/O Parent] WARNING: Failed to launch tab subprocess @SB::LA::SpawnTarget
  (Error:-2147024809): file checkouts\gecko\ipc\glue\GeckoChildProcessHost.cpp:978`
  (also `socket subprocess`, `utility subprocess`). 0x80070057 = invalid parameter. No content
  process exists, so the first tab never gets a window global: longer polling cannot help, and
  neither can `newTab`.
- A standalone Rust harness with no ff-rdp code (`spawnrepro`, below) reproduces it: 1/3, then
  2/80 serial. One bad instance was **partial**: one failed spawn and 7 healthy content
  processes. A single `Failed to launch … subprocess` line does not mean the instance is dead.
- Raw Firefox from PowerShell (`rawff.ps1`, below): 0/150.
- The trigger is not isolated. It still happened with stderr to a file and without
  `-wait-for-browser` (1/130 each), then the rate on this machine fell to zero: 3/75 (~02:00),
  2/80 (~02:25), 1/130 ×3 (~02:40), 0/630 after 02:55 (0/520 four at once, then 0/110 serial
  control at 03:05–03:13). Cause of the spawn failure and of the drift: unknown. Whether CI
  fails for the same reason: unconfirmed (its stderr was redacted); the symptom is identical.
- Separate observation: in the first headless run (`302-nonelev-headless`, 2/20) launches 1 and 3
  failed with `Firefox exited immediately with exit code: 0` (the launcher exited 0 after 400 ms,
  having printed only `*** You are running in headless mode.`); 8–14 s later a Firefox for that
  port appeared under a different, already-gone parent, with no listener. Firefox wrote
  `update_telemetry.json` in the same second, so probably post-update work. Not seen again in
  ~75 later ff-rdp launches (backlog line).
- Launch time, current main, successful headless launches: median 2.5 s (60 runs), 2.4–3.1 s
  (15 runs); the first run after the Firefox update was slower (2.8–5.8 s). Headed: 20/20 ok,
  3.1–4.7 s.

### What ff-rdp does about it (iter-302)

`launch` counts the marker `Failed to launch tab subprocess` in the raw startup stderr it already
captures (the text stays redacted unless `FF_RDP_TRACE_RAW=1`). When `listTabs` still fails with
`currentWindowGlobal is null` 8 s after the first marker, the instance is dead on arrival:
`launch` stops it (its own process tree, managed profile removed), waits for the port to close,
and relaunches once with the same options on a fresh profile. A successful relaunch is reported
in `results.warnings`; a second dead instance fails with "Firefox could not start its content
processes, twice" and both marker counts. See [[iteration-302-windows-listtabs-launch-failure]].

### `rawff.ps1`

```powershell
# Raw Firefox launches without ff-rdp: does the sandbox fail to spawn child
# processes ("SpawnTarget" in stderr, no -contentproc children)?
# Teardown only touches firefox.exe whose command line names this run's profile
# directory: the owner's desktop Firefox runs on this machine.
param(
  [Parameter(Mandatory)][string]$Label,
  [int]$N = 150,
  [ValidateSet('files', 'none')][string]$Stdio = 'files',
  [switch]$NoWaitForBrowser,
  [int]$SettleMs = 4000
)
$ErrorActionPreference = 'Continue'
$ff = 'C:\Program Files\Mozilla Firefox\firefox.exe'
$W = "$env:USERPROFILE\ffrdp-work\runs\$Label"
New-Item -ItemType Directory -Force $W | Out-Null
$summary = Join-Path $W 'summary.jsonl'
Remove-Item $summary -ErrorAction SilentlyContinue
$env:MOZ_CRASHREPORTER_DISABLE = '1'

function Mine([string]$tag) {
  @(Get-CimInstance Win32_Process -Filter "Name='firefox.exe'" |
    Where-Object { $_.CommandLine -like "*$tag*" -and $_.CommandLine -notlike '*-contentproc*' })
}

$bad = 0
for ($i = 1; $i -le $N; $i++) {
  $tag = "rawff-$Label-$i-"
  $prof = Join-Path $W "${tag}profile"
  New-Item -ItemType Directory -Force $prof | Out-Null
  $e = Join-Path $W "ff-$i.err"; $o = Join-Path $W "ff-$i.out"
  $a = @('-no-remote', '--headless')
  if (-not $NoWaitForBrowser) { $a += '-wait-for-browser' }
  $a += @('-profile', $prof)
  if ($Stdio -eq 'files') {
    $p = Start-Process -FilePath $ff -ArgumentList $a -RedirectStandardOutput $o -RedirectStandardError $e -NoNewWindow -PassThru
  } else {
    $p = Start-Process -FilePath $ff -ArgumentList $a -WindowStyle Hidden -PassThru
  }
  Start-Sleep -Milliseconds $SettleMs
  $mine = Mine $tag
  $ids = @($mine | ForEach-Object ProcessId)
  $children = @(Get-CimInstance Win32_Process -Filter "Name='firefox.exe'" |
    Where-Object { $_.CommandLine -like '*-contentproc*' -and $ids -contains $_.ParentProcessId })
  $spawnErr = 0
  if ($Stdio -eq 'files') { $spawnErr = @(Select-String -Path $e -Pattern 'SpawnTarget' -ErrorAction SilentlyContinue).Count }
  $isBad = ($spawnErr -gt 0 -or $children.Count -eq 0)
  if ($isBad) { $bad++ }

  foreach ($q in $mine) { Stop-Process -Id $q.ProcessId -Force -ErrorAction SilentlyContinue }
  $deadline = (Get-Date).AddSeconds(10)
  while ((Mine $tag).Count -gt 0 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
  $left = (Mine $tag).Count

  $rec = [ordered]@{ i = $i; label = $Label; bad = $isBad; mine = $mine.Count; contentprocs = $children.Count; spawn_errors = $spawnErr; left = $left }
  ($rec | ConvertTo-Json -Compress) | Add-Content -Encoding utf8 $summary
  if ($isBad -or $left -gt 0) { "{0,3} BAD mine={1} contentprocs={2} spawn_errors={3} left={4}" -f $i, $mine.Count, $children.Count, $spawnErr, $left }
  if (-not $isBad) { Remove-Item $e, $o -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 300
  Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue
}
"RESULT $Label bad=$bad/$N"
"RAWFF-DONE"
```

### `spawnrepro`

A standalone Cargo crate (no ff-rdp dependency) in `C:\Users\james\ffrdp-work\spawnrepro\`
(`Cargo.toml` with an empty `[workspace]`, `src/main.rs`, 181 lines). It spawns Firefox the way
`launch` does — stdin NUL, stdout NUL, stderr piped, `-no-remote --start-debugger-server <port>
--headless -wait-for-browser --profile <fresh dir with user.js>`, `LANG`/`LC_ALL=en_US.UTF-8`,
`MOZ_CRASHREPORTER_DISABLE=1`, std handles disinherited — one variable at a time, waits
`settle_ms`, counts `-contentproc` descendants and `SpawnTarget` lines, and tears down with
`taskkill /PID <launcher> /T /F` (its own tree only). An instance is bad when it has a spawn
error or no content process.

```text
usage: spawnrepro <label> <n> <base_port> [stdout=null|file] [stderr=piped|null|file]
                  [stdin=null|inherit] [disinherit=yes|no] [wfb=yes|no] [settle_ms=4000] [dbg=yes|no]
```

Runs named `A-ffrdp-like`, `A2-piped` … `G3-no-debugger` under `runs\` are its output.

On #388 the `windows-launch` run 44448665301 (windows-latest) logged the relaunch in scenario C
(headed): the first Firefox's sandbox failed to launch the tab process 18 times, `launch`
relaunched and passed. CI's failure is this one.
