---
name: delegate-to-opencode
description: >
  Delegates implementation heavy-lifting to a model via the opencode CLI
  (default: opencode/nemotron-3-ultra-free, $0 cost -- override with any
  model the user names), while the orchestrating agent plans the task, writes
  the prompt, and reviews and iterates on the worker's output before
  accepting anything -- never trusting opencode's exit code or self-reported
  success without independently checking the diff and running real
  verification. Supports fanning out several isolated opencode worker
  processes in parallel for independent subtasks, each in its own git
  worktree or scratch directory with a fresh session, a unique title, and no
  shared session state, with the orchestrating agent as the sole coordinator.
  Use when the user asks to delegate coding work to opencode, offload
  implementation to a free or local model, run opencode workers in parallel,
  have a worker model do the grunt work while the agent reviews it, use a
  specific model via opencode, delegate on Windows/PowerShell, or runs
  /delegate-to-opencode. On native Windows without WSL, use
  references/windows.md instead of the bash below.
---

# Delegate to opencode

A playbook for an orchestrating coding agent to delegate implementation work
to a model via the `opencode` CLI, while staying in the planner/reviewer role
itself. Ships as a Claude Code skill (this file's frontmatter drives
auto-discovery there); the instructions below are plain and agent-agnostic --
any coding agent that can run bash and follow a written playbook (Codex,
aider, a human at a terminal, ...) can use it the same way. Just treat
everything after this point as your instructions.

The agent stays the planner, prompter, and QA. The model behind `opencode` --
by default the free `opencode/nemotron-3-ultra-free`, but see Model selection
-- does the actual typing: editing files and running commands inside a
directory the agent chose for it. The agent never merges or reports success
without independently checking the result.

## Prerequisites

- `opencode` CLI installed and on `PATH` (https://opencode.ai), with a
  provider authenticated -- `opencode auth login`. For the default free
  model that's the "OpenCode Zen" provider; `opencode providers list` should
  show it as logged in.
- `opencode.json`'s `permission` block set to allow non-interactive use, at
  minimum:
  ```json
  { "permission": { "read": "allow", "edit": "allow", "bash": "allow" } }
  ```
  Without this, opencode will try to prompt for confirmation, which hangs
  forever in a non-interactive/scripted invocation -- there's no human to
  answer it. This is exactly what makes `--dir` the real isolation boundary
  (see Boundaries): the worker gets unsupervised read/edit/bash inside
  whatever directory it's given.
- `git`, `jq`, and standard coreutils (`mktemp`, `awk`, `timeout`) available --
  a real POSIX-ish shell. On Windows this means running inside WSL and using
  this file as-is; on native Windows without WSL, use
  [`references/windows.md`](references/windows.md) instead -- a PowerShell
  translation of every command block below, same rules and reasoning.

## Model selection

Default: `opencode/nemotron-3-ultra-free` -- free, 1,000,000 token context,
128,000 token output limit, supports tool calls and reasoning. Use this
unless the user names a different one.

If the user names a model, use that instead, everywhere `$MODEL` appears
below (any `<provider>/<model-id>` your `opencode` installation supports --
list them with `opencode models`):

```bash
MODEL="opencode/nemotron-3-ultra-free"   # override if the user asked for a specific model
```

Non-default models may not be free, may have different context/output limits
or tool-call support, and may not need the extra-explicit prompting style
recommended below for the free tier -- adjust accordingly. A faster free
sibling also exists, `opencode/nemotron-3.5-lightning-free` (262K/262K
context/output) -- good for many small independent subtasks where turnaround
matters more than context depth, or as a fallback if the default is
persistently erroring.

**Before using a model that isn't a known-free default, check whether it
actually costs money:**

```bash
PROVIDER="${MODEL%%/*}"; MODEL_ID="${MODEL#*/}"
IS_FREE=$(jq -r --arg p "$PROVIDER" --arg m "$MODEL_ID" '
  .[]? | select(.id==$p) | .models[$m]?.cost? |
  if . == null then "unknown"
  elif (.input==0 and .output==0) then "free"
  else "paid" end
' ~/.cache/opencode/models.json 2>/dev/null)
```

If `$IS_FREE` isn't `free` (it came back `paid`, `unknown`, or the cache file
is missing -- try `opencode models --refresh` first if so), tell the user
which model you're about to use, that it isn't confirmed free, and roughly
how many invocations to expect (parallel workers multiply this by worker
count, and iteration can run up to 5 turns per worker) -- then get an
explicit go-ahead before the first real invocation. Skip this check only for
`opencode/nemotron-3-ultra-free` and `opencode/nemotron-3.5-lightning-free`,
which are confirmed free.

## Is this task a fit for delegation?

Delegate when the subtask has:
- A clear, checkable spec (tests to pass, a described behavior, a pattern to replicate).
- Mechanical/boilerplate-heavy implementation: repetitive code, several similar
  files, straightforward plumbing, porting/translating code, writing tests
  against an already-defined interface.
- A diff that's independently verifiable and doesn't depend on work happening
  elsewhere right now.

Do it yourself instead when:
- It's small enough that round-tripping a prompt costs more than just doing it.
- It needs cross-file architectural judgment, ambiguous-requirement calls, or
  repo-wide context that's expensive to hand to another model.
- It touches secrets, credentials, production config/infra, or anything
  destructive/hard to undo -- the worker has unrestricted bash + edit access
  inside whatever directory you give it (see Boundaries).
- Verifying the output would cost as much as doing the work (if reviewing means
  re-deriving the whole solution, delegation added a round-trip for nothing).
- It needs mid-task clarification from the user -- the worker can't ask; only
  the orchestrating agent can.

## Process (applies per worker -- one worker is just N=1)

### 0. Smoke-test opencode (once per session, not per worker)

Before delegating anything real, confirm opencode itself is responding.
Cheap and fast when healthy -- and per the timeout note in step 2, silent
hangs are a real, observed condition worth ruling out before committing a
real task's time budget to it:

```bash
timeout 30 opencode run "Reply with exactly the single word OK and do nothing else. Do not read, create, or edit any files." \
  -m "$MODEL" \
  --format json \
  --pure \
  --dir "$(mktemp -d)" \
  --title "smoke-test" \
  >/tmp/opencode-smoke.json 2>&1
SMOKE_EXIT=$?
```

If `SMOKE_EXIT` is non-zero (including 124), don't proceed with real
delegation yet -- tell the user opencode isn't responding reliably right now
(check `opencode providers list` / `opencode auth login`, or just try again
shortly), rather than discovering the same thing 180 seconds into a real
task. Once this passes, skip it for the rest of the session -- no need to
repeat per worker.

### 1. Scope the task and isolate a workspace

Pick a short, unique `LABEL` for this subtask (e.g. `json-import-fix`). Reuse it
for the branch, the worktree path, and `--title` so everything ties together.
Set `MODEL` here too (see Model selection).

Repo-based task -- isolate with a worktree, kept **outside** the repo so it
never shows up in the user's own `git status`:

```bash
REPO_ROOT=$(git rev-parse --show-toplevel)
LABEL="json-import-fix"
MODEL="opencode/nemotron-3-ultra-free"
WORKTREE_DIR="$(mktemp -d)/$LABEL"
git -C "$REPO_ROOT" worktree add -b "opencode/$LABEL" "$WORKTREE_DIR"
```

Note: a fresh worktree only contains committed history. If the task depends on
the user's current *uncommitted* changes, either get those committed first
(with the user's awareness) or copy them into the worktree explicitly -- don't
assume the worker can see them.

Non-repo / standalone task -- a plain scratch directory is enough:

```bash
WORKTREE_DIR=$(mktemp -d)
```

Keep opencode's own stdout/stderr somewhere else entirely, so it never pollutes
the diff you're about to review:

```bash
RUN_LOG_DIR=$(mktemp -d)
```

### 2. First turn -- always a fresh session, always supervised

**Why not just `timeout N opencode run ...`?** Because a fixed deadline
answers the wrong question. It can't tell "still working, needs more time"
from "wedged and will never finish" -- so you either cut off slow-but-healthy
work, or sit idle for the full deadline after something has already died.
Instead, launch it in the background and supervise it: **keep waiting as long
as it's making progress, kill it once it goes quiet.** Two independent signals
make that possible:

- **Completion:** the `--format json` event stream ends with a `step_finish`
  event whose `part.reason` is `"stop"`. That's the model saying "this turn is
  done" -- the closest thing to the worker reporting back. (Other
  `step_finish` reasons, e.g. `tool-calls`, mean more steps are coming; don't
  treat those as done.)
- **Liveness:** opencode writes a continuous internal log (path from
  `opencode debug paths`, file `opencode.log`) as it boots, creates the
  session, and runs each step -- even before any JSON event reaches stdout.
  Lines still appearing for your run = alive. Lines stopped = stalled.

```bash
LOG_FILE="$(opencode debug paths | awk '$1=="log"{print $2}')/opencode.log"
OUT="$RUN_LOG_DIR/turn1.out.json"
POLL_SECS=5          # how often to check
STALL_SECS=45        # no progress for this long => wedged, kill it
MAX_TOTAL_SECS=600   # absolute backstop, even if it keeps looking busy
LOG_MARK=$(wc -l < "$LOG_FILE")   # ignore everything already in the shared log

turn_complete() {
  awk '/^\{/{f=1} f' "$1" 2>/dev/null \
    | jq -s -e 'any(.[]; .type=="step_finish" and .part.reason=="stop")' >/dev/null 2>&1
}

opencode run "$(cat <<'PROMPT'
Implement <precise, scoped task>. Only touch files under <path>.
Follow the existing conventions in <specific file to mirror>, attached below.
Acceptance: running `<exact test/build command>` in this directory must pass.
Do one focused turn and then stop -- do not schedule or defer any follow-up work.
PROMPT
)" \
  -m "$MODEL" --format json --pure \
  --dir "$WORKTREE_DIR" --title "$LABEL" \
  -f "<path/to/spec-or-example-file>" \
  >"$OUT" 2>"$RUN_LOG_DIR/turn1.err.log" &
PID=$!

RUN_ID=""; START=$SECONDS; LAST_PROGRESS=$SECONDS; LAST_COUNT=0; OUTCOME=""
while :; do
  if ! kill -0 "$PID" 2>/dev/null; then OUTCOME="exited-cleanly"; break; fi
  sleep "$POLL_SECS"

  # Correlate this run in the shared log (it logs our --dir at startup).
  if [ -z "$RUN_ID" ]; then
    RUN_ID=$(tail -n +"$((LOG_MARK + 1))" "$LOG_FILE" 2>/dev/null \
      | grep -F "$WORKTREE_DIR" | grep -oE 'run=[a-f0-9]+' | head -n1 | cut -d= -f2)
  fi
  COUNT=0
  [ -n "$RUN_ID" ] && COUNT=$(tail -n +"$((LOG_MARK + 1))" "$LOG_FILE" 2>/dev/null \
    | grep -c "run=$RUN_ID" || true)
  [ "$COUNT" -gt "$LAST_COUNT" ] && { LAST_COUNT=$COUNT; LAST_PROGRESS=$SECONDS; }

  if turn_complete "$OUT"; then
    sleep 2; kill "$PID" 2>/dev/null; OUTCOME="signalled-complete"; break
  fi
  if [ $((SECONDS - LAST_PROGRESS)) -ge "$STALL_SECS" ]; then
    kill "$PID" 2>/dev/null; OUTCOME="stalled-no-progress"; break
  fi
  if [ $((SECONDS - START)) -ge "$MAX_TOTAL_SECS" ]; then
    kill "$PID" 2>/dev/null; OUTCOME="max-deadline"; break
  fi
done
wait "$PID" 2>/dev/null
```

Correlating on `run=$RUN_ID` (not just "the log grew") is what keeps this
correct with parallel workers -- they all write to the same shared log file,
so a sibling's progress must not read as yours.

`-f` is optional -- attach a spec or an existing file to mirror when that's more
reliable than trusting the model to go find conventions on its own. If you're
on the free tier, write the prompt more explicitly than you would for a
stronger model: exact paths, exact commands, exact scope boundary.

**Acting on `$OUTCOME`:**

| Outcome | What it means | What to do |
|---|---|---|
| `signalled-complete` | Model emitted its terminal event | Go to Review (step 4). Normal path. |
| `exited-cleanly` | Process ended on its own | Go to Review. Check the exit status too, but review the diff either way. |
| `stalled-no-progress` | Wedged -- no log/output movement for `STALL_SECS` | **Check the filesystem before assuming nothing happened** (see below), then decide. |
| `max-deadline` | Looked busy the whole time but never finished | Same: inspect what landed, then decide. |

**Never treat a stall as "no work was done."** Verified: `opencode run` can
hang with no output, no error, and no exit, in two distinct modes --
(a) during **bootstrap**, before the session is even created, so nothing
happens at all; and (b) **mid-turn**, where log inspection showed the model
had already read *and correctly edited* the target file on disk before
stalling on a later internal call. In mode (b) the work is done and sitting in
`$WORKTREE_DIR` -- discarding it because the process misbehaved would throw
away a good result. So on any non-completion outcome, read `$WORKTREE_DIR`
first, then decide. Root cause was never pinned down (ruled out: repo/path
collisions, sandboxed-shell execution, missing session/desktop environment
variables, TTY detection); it looks like the free backend intermittently
stalling with nothing surfaced. Treat it as a known operating condition, not a
bug in this playbook. If nothing landed, don't retry the same call in a loop;
see Parallel workers for how to back off.

Non-interactive shells sometimes print a startup banner (MOTD, `fastfetch`,
etc.) to stdout before your real command's output -- which is why every read
of the stream above goes through `awk '/^\{/{f=1} f'` first. Beyond the
terminal-event check, the exact event schema isn't guaranteed stable across
opencode versions: read the events and interpret them rather than hard-coding
assumptions. And the ground truth for "what actually happened" is always the
filesystem, not the model's narration (see Review).

### 3. Resolve and record the session id (once, right after the first turn)

Do **not** use `-c`/`--continue` for follow-up turns -- it continues the
most-recently-used session across the *entire* local opencode database, which
is ambiguous the moment any other opencode invocation exists (guaranteed with
parallel workers, not even safe to assume for a lone worker). Resolve the real
id instead, matching on both the unique title and directory:

```bash
SESSION_ID=$(opencode session list --format json \
  | awk '/^\[/{f=1} f' \
  | jq -r --arg t "$LABEL" --arg d "$WORKTREE_DIR" \
    '[.[] | select(.title==$t and .directory==$d)]
     | if length==1 then .[0].id else "LOOKUP_FAILED" end')
```

If this returns `LOOKUP_FAILED`, stop and investigate before continuing --
don't guess which session to talk to.

### 4. Review -- never trust the worker's own account

Exit code 0 only means the CLI turn finished. It says nothing about
correctness, and the JSON/text output may contain the model simply claiming
success. Every round, before deciding "satisfied" or "unsatisfied":

1. Read the actual diff yourself: `git -C "$WORKTREE_DIR" add -A && git -C "$WORKTREE_DIR" diff --cached` (or read the produced files directly for a non-repo task).
2. Run the project's real build/lint/test commands inside `$WORKTREE_DIR` and read their actual output/exit code -- don't infer from the model's summary.
3. Confirm the changed files match the requested scope -- nothing touched outside what you asked for.
4. If there's no automated check for this task, read the produced content yourself and reason about correctness. Never accept solely because the process exited 0 or the model said "done."
5. Treat anything suspicious (unexpected network calls, unrelated files touched, dependency changes) as stop-and-flag, not something to wave through -- the worker has unrestricted bash + edit inside `$WORKTREE_DIR` (see Boundaries).

### 5. Iterate, bounded

Up to **5 total turns per worker** (1 initial + up to 4 refinements). Refinement
prompts should be short and specific -- the session already has full prior
context, so name the exact failure (quote the failing test, point at the exact
diff problem), not "try again":

```bash
OUT="$RUN_LOG_DIR/turn2.out.json"
LOG_MARK=$(wc -l < "$LOG_FILE")   # re-mark: only this turn's log lines count

opencode run "The tests in <path> still fail because <specific reason>. Fix specifically that -- don't change anything else." \
  -m "$MODEL" --format json --pure \
  --dir "$WORKTREE_DIR" --session "$SESSION_ID" --title "$LABEL" \
  >"$OUT" 2>"$RUN_LOG_DIR/turn2.err.log" &
PID=$!
# ...then the same supervise loop as step 2, unchanged.
```

Re-marking `LOG_MARK` and resetting `RUN_ID`/`LAST_COUNT` before each turn
matters: a new invocation gets a new run id, and stale counters from the
previous turn would make a fresh process look like it had already stalled.

If round 5 still isn't acceptable: stop delegating this subtask. Either finish
it yourself directly, or tell the user plainly what was tried and why it isn't
converging -- never silently give up and never claim success that didn't happen.

### 6. Integrate or discard

Once satisfied:

```bash
git -C "$WORKTREE_DIR" add -A
git -C "$WORKTREE_DIR" diff --cached > "$RUN_LOG_DIR/final.diff"
git -C "$REPO_ROOT" apply "$RUN_LOG_DIR/final.diff"
```

If `git apply` fails (conflicts, renames), fall back to
`git -C "$REPO_ROOT" apply --3way "$RUN_LOG_DIR/final.diff"`, or inspect
`final.diff` and apply the changes manually -- don't force it silently.

Either way, clean up (`--force` is required: the worktree only ever has
staged-but-uncommitted changes at this point, so a plain `remove` refuses):

```bash
git -C "$REPO_ROOT" worktree remove --force "$WORKTREE_DIR"
git -C "$REPO_ROOT" branch -D "opencode/$LABEL"   # optional, if you don't want it lingering
```

For a non-repo scratch task, just copy out whatever's worth keeping and
`rm -rf "$WORKTREE_DIR"`.

## Parallel workers

Each independent subtask gets its own `LABEL`, its own worktree/scratch dir
(step 1), its own session (steps 2-3) -- never shared between workers. Launch
the first-turn `opencode run` calls together in the background and track them
independently -- e.g. Claude Code's Bash tool `run_in_background: true`
(relying on its own completion notifications instead of hand-rolling a poll
loop) if that's available, or plain shell backgrounding otherwise
(`cmd & pid=$!; ...; wait "$pid"`). Then run steps 3-6 independently per
worker as each finishes.

Concurrency cap: **default 3 concurrent workers, soft ceiling 4.** There is no
documented rate limit for the free tier to design against -- this is a
deliberate, conservative heuristic, not a confirmed number. Treat the first
real parallel batch as calibration. If any worker ends in `stalled-no-progress`
or `max-deadline` (infrastructure-level failure, not a bad answer -- see
step 2), reduce concurrency for the rest of the queued subtasks in this batch
rather than blindly relaunching the same call.

Each worker needs its **own** `LOG_MARK`, `RUN_ID`, and progress counters --
they share one log file, and correlating on `run=$RUN_ID` is what stops one
worker's progress from masking another's stall.

Review and integrate each worker's diff independently (step 4 and 6) before
touching the real branch -- don't batch-merge unreviewed diffs just because
they all finished.

**Isolation verified in practice:** two workers launched together (distinct
repos, dirs, sessions, titles) ran their full timeout windows independently
with zero cross-talk -- each resolved to its own session, neither touched the
other's files, and each was correctly identifiable afterward purely from its
own `LABEL`/`WORKTREE_DIR`. Completion reliability is a separate question and
inherits the same caveat as a lone worker (see the timeout note in step 2) --
isolation being solid doesn't make the underlying model faster or more
reliable, it just guarantees failures stay contained to the worker that hit
them instead of corrupting its siblings.

## Recovering orphaned workers

If a previous delegation was interrupted (agent crashed, session ended, hard
timeout) before step 6 ran, its worktree and branch are left behind. Nothing
auto-deletes them -- deleting a worktree you haven't inspected could throw
away completed work (see the timeout note in step 2). Check for orphans at
the start of a delegation session, or whenever you suspect a previous one
didn't finish cleanly:

```bash
git -C "$REPO_ROOT" worktree list | grep 'opencode/'
```

For each orphan found: inspect it like step 4 (`git -C <dir> diff`, read the
files), then either integrate it (step 6) or discard it
(`git -C "$REPO_ROOT" worktree remove --force <dir>` +
`git -C "$REPO_ROOT" branch -D <branch>`). Don't bulk-delete without looking
-- same "never trust blindly" rule as reviewing a worker's own output.

Non-repo scratch-dir tasks aren't tracked anywhere -- an interrupted one just
leaves an orphaned directory under the system temp dir, lower-stakes and
typically cleaned up by the OS over time.

## Boundaries

- Always pass `--pure` on every invocation, first turn and every continuation.
  Some opencode installations have external plugins configured (the `plugin`
  list in `opencode.json`), and some plugins grant the model tools to
  re-inject prompts into a session later, on a timer, unsupervised. `--pure`
  disables all external plugins for that invocation regardless of what's
  configured, keeping every turn strictly "respond once, then stop" and the
  orchestrating agent the sole decider of the next turn.
- Never use `-c`/`--continue`. Always resolve and pass an explicit `-s <id>`.
- Never use `--attach` -- it reuses an already-running server, defeating the
  "each invocation gets its own ephemeral, isolated server on a random port"
  property that concurrent workers rely on.
- Never use `--auto` -- it's redundant with a correctly configured permission
  block (see Prerequisites), and the CLI's own `--help` text calls it
  dangerous.
- Never use `--share` -- don't publish the user's code/prompts externally.
- Never use `-i`/`--interactive` -- this skill only drives opencode
  non-interactively.
- Never point `--dir` at the user's real working directory/branch, their home
  directory, or anything broader than the task needs. Per Prerequisites, the
  worker gets unsupervised read/edit/bash inside `--dir` with zero
  confirmation prompts -- the isolation boundary *is* `--dir`.
- Never let two concurrently running workers share a `--dir` or a session id.
- Don't retry a failed `opencode run` invocation in a loop. Surface the
  failure, decide deliberately (fewer workers, different approach, or hand it
  to the user), and re-issue as a new, visible action if warranted.
- Never run `opencode run` unsupervised. It has been observed to hang
  indefinitely with no output, no error, and no exit -- both during bootstrap
  and mid-turn. A hang is not self-limiting, so a stuck worker would occupy
  its slot forever. Use the supervise loop from step 2 (or at minimum a
  `timeout` wrapper), and always check `$WORKTREE_DIR` for completed work
  before treating a kill as wasted effort -- the edit may have already landed.

## Flags reference

| Flag | Use | Why |
|---|---|---|
| `-m "$MODEL"` | Always | Defaults to `opencode/nemotron-3-ultra-free` (free, 1M/128K context/output, tool calls + reasoning) -- see Model selection for overriding it. |
| `--format json` | Always | Scripting-friendly NDJSON-style events, avoids the human transcript's TTY/color formatting noise. |
| `--pure` | Always | Disables external plugins for this invocation. |
| `--dir <worktree-or-scratch>` | Always | Isolation boundary; never the user's primary working tree. |
| `--title <LABEL>` | Always | Unique per worker; used to resolve the session id afterward. |
| `-s/--session <id>` | Every turn after the first | Explicit, unambiguous continuation. |
| `-f/--file <path>` | Optional | Attach a spec/example file directly rather than relying on the model to find conventions itself. |
| `-c/--continue` | Never | Global "most-recently-used session" pointer -- a race the moment more than one opencode invocation exists. |
| `--fork` | Not by default | Available if you deliberately want to branch one session's history into two independent continuations; not needed for the core loop. |
| `--attach` | Never | Forces a shared server, breaking per-invocation isolation. |
| `--auto` | Never | Redundant given a correctly configured permission block; the CLI's own help calls it dangerous. |
| `--share` | Never | Would publish the session externally. |
| `-i/--interactive` | Never | This skill is non-interactive only. |
| `--port` | Never (leave default) | Default is a random free port per invocation -- exactly what concurrent workers need; a fixed port would collide. |

## Deliberate omissions

- **No `tools:` frontmatter field** -- the documented fallback ("finish it
  yourself directly") means the agent may need Edit/Write/Read/Glob/Grep in
  addition to Bash; restricting to a guessed subset would break that fallback
  for no actual benefit.
- **No `model:` frontmatter field** -- nothing about the orchestrator role
  requires pinning the orchestrating agent itself to a different model than
  whatever the user is already running. (This is unrelated to `$MODEL`, which
  only selects the model *opencode* uses for the delegated work.)
- **No `scripts/` wrapper** -- the command set is a handful of flag variations
  on one CLI, not reusable logic.
