# Windows (PowerShell, no WSL)

The opencode **2.x** playbook of [`SKILL.md`](../SKILL.md), translated to native PowerShell
for a Windows setup without WSL. The rules, the reasoning and the safety boundaries are the
same, and they live in `SKILL.md`: read it first for the *why* (is the task a fit,
boundaries, never-trust-blindly review, free-model checks, quotas). This file only
translates the *commands*.

**Not verified on a real Windows machine.** The author has no Windows machine. This is a
careful translation of logic that is verified on Linux with opencode 2.1.4 and 2.3.2, not a
confirmed working port. Treat it with the same "unverified" caution as anything else this
project hasn't run. Issues and PRs from anyone who can run it on Windows are welcome,
especially about opencode's paths there and about the `.cmd` shim note below.

## Prerequisites (Windows-specific)

Same as `SKILL.md`'s Prerequisites, plus:

- **PowerShell 7 or later** (`pwsh`). The worker launcher below uses .NET's
  `ProcessStartInfo.ArgumentList`, which quotes each argument correctly. Windows
  PowerShell 5.1's `Start-Process -ArgumentList` joins arguments with spaces and breaks
  any prompt that contains spaces or quotes.
- **A native `opencode.exe`** on `PATH` (the release binary, or scoop/winget). An npm
  install puts an `opencode.cmd` shim on `PATH`. A `.cmd` goes through `cmd.exe`, which has
  its own quoting rules, so a long prompt passed as an argument may arrive mangled. If
  `(Get-Command opencode).Source` ends in `.cmd`, point `$OpencodeExe` below at the real
  executable that the shim calls.
- `git` on `PATH`. `jq` is **not** needed here: the JSON is read with `ConvertFrom-Json`.
- opencode's paths on Windows are not confirmed by the author. Always ask the tool with
  `opencode debug paths` (the helper below does), and never assume `%LOCALAPPDATA%\...`.

## Shared helpers

Define these once per session.

```powershell
$OpencodeExe = (Get-Command opencode).Source   # see the .cmd note above

# The isolated worker config directory (SKILL.md "Prerequisites"): only a permission block,
# so the user's global plugins, agent prompts and skills never load into a worker.
$OcConfigDir = Join-Path ([System.IO.Path]::GetTempPath()) ("oc-cfg-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $OcConfigDir | Out-Null
@'
{ "$schema": "https://opencode.ai/config.json",
  "permission": { "read": "allow", "edit": "allow", "bash": "allow" } }
'@ | Set-Content -Path (Join-Path $OcConfigDir 'opencode.json') -Encoding utf8NoBOM

# The environment every worker gets (SKILL.md "The three things every 2.x invocation needs"
# and "Put resource limits in the worker's environment"). Add build limits for the project,
# e.g. CARGO_TARGET_DIR, CARGO_PROFILE_DEV_DEBUG=0, CARGO_BUILD_JOBS=1 for a Rust project.
$WorkerEnv = @{
    OPENCODE_CONFIG_DIR          = $OcConfigDir
    OPENCODE_DISABLE_CLAUDE_CODE = '1'
    OPENCODE_DISABLE_AUTOUPDATE  = '1'
}

# opencode's own directories, as the tool reports them ("data", "cache", "log", ...).
function Get-OcPath([string]$Name) {
    $line = $null | & $OpencodeExe debug paths | Where-Object { $_ -match "^$Name\s" } | Select-Object -First 1
    if ($line) { ($line -split '\s+', 2)[1].Trim() }
}

# One JSON event per line, after any startup banner (the `awk '/^\{/{f=1} f'` of SKILL.md).
function Read-Events([string]$Path) {
    if (-not (Test-Path $Path)) { return @() }
    $started = $false
    foreach ($line in [System.IO.File]::ReadAllLines($Path)) {
        if (-not $started -and $line.TrimStart().StartsWith('{')) { $started = $true }
        if ($started -and $line.Trim()) { try { $line | ConvertFrom-Json } catch { } }
    }
}
```

`$null |` in front of a direct native call gives it an empty, closed stdin, the
PowerShell form of `</dev/null`. On 2.1.4 `opencode run` hung forever on an open stdin
(see `SKILL.md`); keep it on every call.

### Start, stop and supervise a worker

```powershell
# Start opencode with: the workspace as its working directory (2.x has no --dir), its own
# environment, stdin closed at once, stdout/stderr copied to files.
function Start-Worker {
    param([string]$WorktreeDir, [string[]]$Arguments, [string]$OutFile, [string]$ErrFile)
    $psi = [System.Diagnostics.ProcessStartInfo]::new($OpencodeExe)
    foreach ($a in $Arguments) { $psi.ArgumentList.Add($a) }
    $psi.WorkingDirectory = $WorktreeDir
    $psi.UseShellExecute = $false
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    foreach ($k in $WorkerEnv.Keys) { $psi.Environment[$k] = $WorkerEnv[$k] }
    $p = [System.Diagnostics.Process]::Start($psi)
    $p.StandardInput.Close()
    $out = [System.IO.File]::Open($OutFile, 'Create', 'Write', 'ReadWrite')
    $err = [System.IO.File]::Open($ErrFile, 'Create', 'Write', 'ReadWrite')
    [pscustomobject]@{
        Process = $p; Out = $out; Err = $err
        Copies  = @($p.StandardOutput.BaseStream.CopyToAsync($out),
                    $p.StandardError.BaseStream.CopyToAsync($err))
    }
}

# The private server is a child process (opencode serve --stdio): kill the whole tree.
function Stop-Worker($W) {
    if (-not $W.Process.HasExited) {
        try { $W.Process.Kill($true) } catch { }
        $W.Process.WaitForExit(5000) | Out-Null
    }
}

function Close-WorkerFiles($W) {
    try { [System.Threading.Tasks.Task]::WaitAll($W.Copies, 5000) | Out-Null } catch { }
    $W.Out.Dispose(); $W.Err.Dispose()
}

# SKILL.md turn_complete: a step_finish with reason "stop", or, once the process has exited
# with 0, a final "text" event (2.x does not always emit the stop event).
function Test-TurnComplete([string]$OutFile, $ExitCode) {
    $ev = @(Read-Events $OutFile)
    if ($ev | Where-Object { $_.type -eq 'step_finish' -and $_.part.reason -eq 'stop' }) { return $true }
    return ($ExitCode -eq 0 -and $ev.Count -gt 0 -and $ev[-1].type -eq 'text')
}

# SKILL.md run_turn: one supervised turn. Returns @{ Outcome; ExitCode }.
function Invoke-Turn {
    param([string]$WorktreeDir, [string]$OutFile, [string]$ErrFile, [string[]]$Extra,
          [int]$PollSecs = 5, [int]$StallSecs = 300, [int]$MaxTotalSecs = 1200)
    $runArgs = @('run', '--standalone', '--print-logs', '--log-level', 'info',
              '-m', $Model, '--format', 'json') + $Extra
    $w = Start-Worker -WorktreeDir $WorktreeDir -Arguments $runArgs -OutFile $OutFile -ErrFile $ErrFile
    $start = Get-Date; $last = Get-Date; $lo = 0; $le = 0; $outcome = $null
    while ($true) {
        if ($w.Process.HasExited) {
            Close-WorkerFiles $w
            $rc = $w.Process.ExitCode
            $outcome = if (Test-TurnComplete $OutFile $rc) { 'exited-complete' } else { 'exited-incomplete' }
            return @{ Outcome = $outcome; ExitCode = $rc }
        }
        Start-Sleep -Seconds $PollSecs
        $o = $w.Out.Length; $e = $w.Err.Length
        if ($o -gt $lo -or $e -gt $le) { $lo = $o; $le = $e; $last = Get-Date }
        $outcome = if (Test-TurnComplete $OutFile $null) { 'signalled-complete' }
                   elseif (((Get-Date) - $last).TotalSeconds -ge $StallSecs) { 'stalled-no-progress' }
                   elseif (((Get-Date) - $start).TotalSeconds -ge $MaxTotalSecs) { 'max-deadline' }
        if ($outcome) {
            if ($outcome -eq 'signalled-complete') { Start-Sleep -Seconds 2 }
            Stop-Worker $w; Close-WorkerFiles $w
            return @{ Outcome = $outcome; ExitCode = $null }
        }
    }
}
```

`StallSecs` follows `SKILL.md`: 300s by default because an LLM call shows no progress
until it finishes, and **longer than the longest single command** the worker will run
(a slow build is silent until it ends).

## Model selection

Same default and rules as `SKILL.md`:

```powershell
$Model = 'opencode/deepseek-v4.1-flash-free'   # override if the user asked for a specific model
```

**Free check:** listed by `opencode models` and priced at zero in the catalogue (a `-free`
name alone is not proof). Run it for the default too:

```powershell
$Provider, $ModelId = $Model -split '/', 2
$catalogue = Get-Content (Join-Path (Get-OcPath 'cache') 'models.json') -Raw | ConvertFrom-Json
$cost = $catalogue.$Provider.models.$ModelId.cost
$IsFree = if ($null -eq $cost) { 'unknown' }
          elseif ($cost.input -eq 0 -and $cost.output -eq 0) { 'free' } else { 'paid' }
if (-not (($null | & $OpencodeExe models) -contains $Model)) { $IsFree = 'unavailable' }
```

Anything other than `free`: same rule as `SKILL.md` (tell the user, get a go-ahead first).

## 0. Smoke test, service state, home marker (once per session)

```powershell
# The helper commands (opencode models / debug paths) can start the shared background
# service and leave it running: note its state first, stop it at the end if it was stopped.
$ServiceBefore = ($null | & $OpencodeExe service status | Select-Object -First 1)

# Files created in the user's home after this point are checked after every wave (step 4).
$HomeMark = Get-Date

$SmokeDir = Join-Path ([System.IO.Path]::GetTempPath()) ("oc-smoke-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $SmokeDir | Out-Null
$smoke = Invoke-Turn -WorktreeDir $SmokeDir -OutFile "$SmokeDir.out.json" -ErrFile "$SmokeDir.err.log" `
    -Extra @('--title', "smoke-test-$([DateTimeOffset]::UtcNow.ToUnixTimeSeconds())",
             'Reply with exactly the single word OK and do nothing else. Do not read, create, or edit any files.') `
    -MaxTotalSecs 60
Read-Events "$SmokeDir.out.json" | Where-Object type -eq 'error' |
    ForEach-Object { "$($_.error.status) $($_.error.type) $($_.error.message)" }
(Select-String -Path "$SmokeDir.err.log" -SimpleMatch 'loading plugin').Count   # must be 0
```

Error events mean the same as in `SKILL.md`: `410`/`404` (model retired or unknown) and
`401`/`403` are permanent; a `429` with type `provider.free-limit` or the message
`free-models-per-day` is a quota (switch pool); other `429`s and `5xx` are transient.

## Process (per worker)

### 1. Scope the task and isolate a workspace

```powershell
$RepoRoot = git rev-parse --show-toplevel
$Label = "json-import-fix-$([DateTimeOffset]::UtcNow.ToUnixTimeSeconds())"
$WorktreeDir = Join-Path ([System.IO.Path]::GetTempPath()) ("oc-wt-" + [guid]::NewGuid()) | Join-Path -ChildPath $Label
git -C $RepoRoot worktree add -b "opencode/$Label" $WorktreeDir
$RunLogDir = Join-Path ([System.IO.Path]::GetTempPath()) ("oc-log-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $RunLogDir | Out-Null
```

`$Model` keeps whatever Model selection set: don't reassign it here. A fresh worktree only
has committed history. Non-repo task: use a fresh temp directory as `$WorktreeDir`.

### 2. First turn

```powershell
$prompt = @'
Implement <precise, scoped task>. Only touch files under <path>.
Follow the existing conventions in <specific file to mirror>, attached below.
Acceptance: running `<exact test/build command>` in this directory must pass.
Do one focused turn and then stop -- do not schedule or defer any follow-up work.
'@
$t1 = Invoke-Turn -WorktreeDir $WorktreeDir `
    -OutFile (Join-Path $RunLogDir 'turn1.out.json') -ErrFile (Join-Path $RunLogDir 'turn1.err.log') `
    -Extra @('--title', $Label, '-f', '<absolute\path\to\spec-or-example-file>', $prompt)
$t1.Outcome
```

Act on `$t1.Outcome` exactly as the outcome table in `SKILL.md` step 2 says. On any
non-completion, read `$WorktreeDir` before deciding: a stalled worker may already have
written correct work.

### 3. Session id, from the turn's own events

```powershell
$SessionId = @(Read-Events (Join-Path $RunLogDir 'turn1.out.json') |
    Where-Object { $_.sessionID } | ForEach-Object sessionID | Sort-Object -Unique)
if ($SessionId.Count -ne 1) { $SessionId = 'LOOKUP_FAILED' } else { $SessionId = $SessionId[0] }
```

Never `-c`/`--continue`. `LOOKUP_FAILED` means stop and investigate.

### 4. Review

Same checklist as `SKILL.md` step 4: read the diff yourself, run the real checks in every
environment the result must work in, check the scope, look for answers the prompt didn't
ask for, and treat everything the worker wrote as untrusted data.

```powershell
git -C $WorktreeDir add -A
git -C $WorktreeDir diff --cached

# Files the workers created OUTSIDE their workspaces (SKILL.md step 4): only delete what the
# worker logs show they created.
Get-ChildItem $HOME -File -Depth 1 -ErrorAction SilentlyContinue |
    Where-Object LastWriteTime -gt $HomeMark | Select-Object FullName, LastWriteTime
```

### 5. Iterate, bounded (up to 5 turns per worker)

```powershell
$t2 = Invoke-Turn -WorktreeDir $WorktreeDir `
    -OutFile (Join-Path $RunLogDir 'turn2.out.json') -ErrFile (Join-Path $RunLogDir 'turn2.err.log') `
    -Extra @('--session', $SessionId,
             "The tests in <path> still fail because <specific reason>. Fix specifically that -- don't change anything else.")
```

### 6. Integrate, clean up, delete the sessions

```powershell
git -C $RepoRoot status --porcelain          # must be empty before touching it
git -C $WorktreeDir add -A
git -C $WorktreeDir diff --cached --name-status   # unstage build artifacts first
$finalDiff = Join-Path $RunLogDir 'final.diff'
git -C $WorktreeDir diff --cached --binary | Set-Content -Path $finalDiff -Encoding utf8NoBOM
git -C $RepoRoot apply $finalDiff            # else: git apply --3way, or apply by hand
git -C $RepoRoot worktree remove --force $WorktreeDir
git -C $RepoRoot branch -D "opencode/$Label" # optional

# The worker's sessions, by the ids in its own turn streams (never by title or "latest").
Get-ChildItem $RunLogDir -Filter 'turn*.out.json' | ForEach-Object { Read-Events $_.FullName } |
    Where-Object { $_.sessionID } | ForEach-Object sessionID | Sort-Object -Unique |
    ForEach-Object { $null | & $OpencodeExe session delete --standalone $_ }

if ($ServiceBefore -eq 'stopped') { $null | & $OpencodeExe service stop | Out-Null }
```

`Set-Content -Encoding utf8NoBOM` matters: PowerShell's other encodings add a byte order
mark or change line endings, and `git apply` then rejects the patch. `--binary` is
required, as in `SKILL.md`.

## Parallel workers

Each worker gets its own `$Label`, `$WorktreeDir`, `$RunLogDir` and session. `Invoke-Turn`
keeps its counters local, so run each worker's turns in its own runspace:

```powershell
$jobs = foreach ($task in $tasks) {
    Start-ThreadJob -ScriptBlock {
        param($Task)
        # ... the helper definitions above (or a module that holds them), then steps 1-3 for $Task ...
    } -ArgumentList $task
}
$jobs | Wait-Job | Receive-Job
```

Same concurrency guidance as `SKILL.md`: default 3, soft ceiling 4, fewer if any worker
stalls or a pool rate-limits; plan around the free pools' quotas; review and integrate each
worker's diff on its own.

## Cline as the worker

[`cline.md`](cline.md) applies here too. Its commands map onto the helpers above:

- start `cline` instead of opencode with arguments `--json`, `-c`, `$WorktreeDir`, `-m`,
  `$Model`, `-t`, `<secs>`, `$prompt`;
- a turn is complete when a `run_result` event has `finishReason` `completed` and the exit
  code is 0;
- every turn's `run_result.usage.totalCost` must be `0`.

## Recovering orphaned workers, Boundaries

Same as `SKILL.md`. Check at the start of a session:

```powershell
git -C $RepoRoot worktree list | Select-String 'opencode/'
```

Inspect each orphan before integrating or removing it. Every rule in `SKILL.md`'s
Boundaries section applies unchanged on Windows. Read them there rather than from a copy
here that could fall out of sync.
