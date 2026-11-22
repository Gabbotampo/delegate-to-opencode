# Cline as the worker CLI

Same playbook, same rules as [`SKILL.md`](../SKILL.md): scope the task, isolate a
workspace, supervise every turn, review the result yourself, never trust the worker's
own account. Read `SKILL.md` first for the *why*. This file only covers what differs
when the worker is the [Cline](https://cline.bot) CLI (`cline`) instead of `opencode`.

Verified on Cline CLI 3.0.73-3.0.75, Linux, 2026-11-22.

## Prerequisites

- `cline` on `PATH` and an authenticated account: the user runs `cline auth` (never
  the agent: it's the user's login). A run with an expired login ends at once with
  `"cline requires re-authentication."` on stderr.
- `jq`, `git`, coreutils, as for opencode.

## Free models only

Cline's own free models are listed by its public feed:

```bash
curl -s --max-time 15 https://api.cline.bot/api/v1/ai/cline/recommended-models \
  | jq -r '.free[].id'
```

Only ids under `.free` are free. `.clinePass` models need a paid Cline Pass, and
`.recommended` ones are paid provider models. Free ids come and go. Observed on
2026-11-22: `cline-free/mimo-v2.6-flash`, `cline-free/deepseek-v4.1-flash`,
`cline-free/kat-coder-pro`, `cline-free/solar-mini4` and
`cline-free/muse-spark-1.3-contributor` all ended with
`"Free model promotion ended"` and `model not found`. The feed's free list was then
`cline-free/nemotron-3.5-super-vl` and `nvidia/nemotron-3.5-lightning-free`. So read
the feed at the start of every delegation session, never a remembered id.

Cline's free models also have a **daily limit per account**: observed, after
about a dozen worker turns in one morning every run ended with
`Daily free model limit reached` (exit 1). Treat it like a quota (switch to
another pool), not like a transient error.

Every run also reports what it cost. **Check it after each turn**: anything other
than `0` means the model was not free. Stop and tell the user.

```bash
jq -r 'select(.type=="run_result") | .usage.totalCost' "$OUT"   # must print 0
```

Default worker model: the first id of the feed's `.free` list
(`cline-free/nemotron-3.5-super-vl` on 2026-11-22: a correct two-step file task in 8s).

## What a worker inherits (isolation)

Cline keeps its login under its configuration directory (`--config`, default
`~/.cline`, data in `~/.cline/data`). A clean `--config` therefore also drops the login:
observed, a run with `--config <empty dir>` ended with `"Unauthorized: ... re-authenticate"`.
Copying the user's credential files into a worker config is not an option: they are
the user's secrets. So **a Cline worker runs with the user's own Cline state**:

- installed plugins (`~/.cline/plugins/_installed/`). Check what is there before
  delegating. The official `goal` plugin only acts after a `/goal` command, which a
  worker prompt never sends. Any plugin that can queue prompts or act on its own is a
  reason not to delegate to Cline on that machine;
- MCP servers (`~/.cline/data/settings/cline_mcp_settings.json`). With any configured,
  the worker can call them;
- hooks (`~/.cline/hooks`). `--hooks-dir` only *adds* a directory, it doesn't replace
  the default;
- rules files the workspace itself carries (`.clinerules`), as with any repository.

A prompt-mode run is fully auto-approved: the `run_start` event says `"mode":"yolo"`.
The worker can read, edit and run commands anywhere your user can. The working
directory is where its edits land by default, but it is not a sandbox. The same
"Is this task a fit" filter as in `SKILL.md` applies, more strictly.

## One supervised turn

Cline's `-t` is a fixed timeout, so wrap it in the same progress-aware supervisor as
`run_turn` in `SKILL.md`. Only the command and the completion test differ:

```bash
POLL_SECS=5; STALL_SECS=300; MAX_TOTAL_SECS=1200

# $1 = event stream, $2 = exit code once exited (empty while running)
cline_turn_complete() {
  [ "$2" = 0 ] && jq -s -e 'any(.[]; .type=="run_result" and .finishReason=="completed")' "$1" >/dev/null 2>&1
}

# run_cline_turn OUT ERR PROMPT -- one supervised turn in $WORKTREE_DIR with $MODEL.
run_cline_turn() {
  local out=$1 err=$2 prompt=$3
  ( cd "$WORKTREE_DIR" && exec cline --json -c "$WORKTREE_DIR" -m "$MODEL" \
      -t "$MAX_TOTAL_SECS" "$prompt" ) >"$out" 2>"$err" </dev/null &
  local pid=$! start=$SECONDS last=$SECONDS lo=0 le=0 o e
  OUTCOME=""; RC=""
  while :; do
    if ! kill -0 "$pid" 2>/dev/null; then
      wait "$pid"; RC=$?
      if cline_turn_complete "$out" "$RC"; then OUTCOME="exited-complete"
      else OUTCOME="exited-incomplete"; fi
      return
    fi
    sleep "$POLL_SECS"
    o=$(wc -c <"$out" 2>/dev/null || echo 0); e=$(wc -c <"$err" 2>/dev/null || echo 0)
    if [ "$o" -gt "$lo" ] || [ "$e" -gt "$le" ]; then lo=$o; le=$e; last=$SECONDS; fi
    if [ $((SECONDS - last)) -ge "$STALL_SECS" ]; then
      stop_worker "$pid"; OUTCOME="stalled-no-progress"; return
    fi
    if [ $((SECONDS - start)) -ge "$MAX_TOTAL_SECS" ]; then
      stop_worker "$pid"; OUTCOME="max-deadline"; return
    fi
  done
}
```

(`stop_worker` is the one from `SKILL.md`.) The event stream is one JSON object per
line: `run_start`, many `agent_event`s (`content_start`, `content_end`,
`iteration_start`, `usage`, ...), then one `run_result` with `finishReason`, `usage`
(including `totalCost`) and `text`, the model's final message. Errors arrive as
`{"type":"error","message":...}` on stderr. Closing stdin (`</dev/null`) isn't needed
for Cline to start (it didn't hang on an inherited stdin), but keep it: piped stdin
is read as prompt input.

## No continuation: refine with a fresh, self-contained turn

`--id <session-id>` resumes only interactively. In JSON mode it ends with
`"JSON output mode requires a prompt argument or piped stdin (interactive mode is
unsupported)"`, with the prompt given as an argument or on stdin (both observed).
So a refinement is a **new** session in the same `$WORKTREE_DIR`. Its prompt must
carry the context the first turn had: the task, what is already in the directory
(the worker can read the files), and the exact defect to fix. The 5-turn bound per
worker from `SKILL.md` still applies.

To record which session a turn was (for the user, or to export it):

```bash
cline history --json </dev/null | jq -r --arg c "$WORKTREE_DIR" '[.[] | select(.cwd==$c)] | .[-1].sessionId'
```

## Flags

| Flag | Use | Why |
|---|---|---|
| `--json` | Always | One event per line; the human view needs a TTY. |
| `-c "$WORKTREE_DIR"` | Always | The worker's working directory. |
| `-m "$MODEL"` | Always | An id from the feed's `.free` list. |
| `-t <secs>` | Always | Backstop timeout; the supervisor above does the real watching. |
| `--config <dir>` | Never | A clean one drops the user's login (see Isolation). |
| `--id` | Not usable | Interactive only. |
| `-y/--yolo` | Not needed | Prompt mode already auto-approves (`"mode":"yolo"`). |
| `--worktree` | Not by default | Creates its own detached worktree under `~/.cline/worktrees/`; this playbook makes its own (step 1), so the review and cleanup steps know where it is. |
| `-i/--tui`, `--acp`, `-z/--zen`, `--kanban` | Never | Interactive, editor, background-hub or UI modes. |
