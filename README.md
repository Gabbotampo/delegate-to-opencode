# delegate-to-opencode

An agent playbook for delegating implementation work to cheaper worker models
while the orchestrating agent stays in the planner/reviewer role. The
orchestrator scopes the task, writes the prompt, and reviews and iterates on
the worker's output before accepting anything. It never trusts the worker
CLI's exit code or the model's own claims of success without checking the
diff itself and running real verification.

Two worker CLIs are supported:

- [`opencode`](https://opencode.ai) 2.x: [`SKILL.md`](SKILL.md);
- the [Cline](https://cline.bot) CLI: [`references/cline.md`](references/cline.md),
  the same playbook with Cline's commands.

Workers run on **free models by default**. A model counts as free only when
the tool's own data says so: for opencode, zero cost in its model catalogue
and listed as available on your machine; for Cline, on Cline's `.free` list,
and every turn's `totalCost` must come back `0`. Paid models need your explicit
go-ahead, with the expected number of calls stated first.

It also fans out several isolated workers in parallel for independent
subtasks. Each worker gets its own git worktree or scratch directory and a
fresh session, with no shared state, and the orchestrating agent is the sole
coordinator.

## What's in the repo

| File | What it is |
|---|---|
| [`SKILL.md`](SKILL.md) | The playbook, with opencode as the worker. Claude Code skill frontmatter, agent-agnostic body. |
| [`references/cline.md`](references/cline.md) | What changes when the worker is Cline: commands, free models, what a worker inherits, refinements. |
| [`references/windows.md`](references/windows.md) | PowerShell 7 translation of the opencode 2.x playbook (untested on a real Windows machine). |

## Install

**Claude Code:**

```bash
git clone https://github.com/Gabbotampo/delegate-to-opencode.git
ln -s "$(pwd)/delegate-to-opencode" ~/.claude/skills/delegate-to-opencode
```

Clone it wherever you keep your repos -- the symlink target is what matters,
not the clone location. Claude Code then picks it up automatically -- ask it
to delegate something to opencode or Cline, or run `/delegate-to-opencode`.

**Anything else (Codex, aider, a plain terminal session, ...):**

Point your agent at `SKILL.md` (or paste its contents into context) and treat
it as your instructions. The frontmatter block at the top is Claude-Code-only
metadata; skip straight to the `# Delegate to opencode` heading.

## Prerequisites

- **opencode 2.x**, authenticated (`opencode auth login`). Verified on 2.1.4
  and 2.3.2. For opencode 1.x (where `opencode run` still has `--pure` and
  `--dir`), use the 1.x revision of `SKILL.md`, commit `d937f69`.
  You don't need to change your own opencode config. Each worker gets a
  separate, throwaway config directory: it allows non-interactive
  `read`/`edit`/`bash` and keeps your global plugins, agent prompts and
  skills out (see `SKILL.md` for why).
- **Or the Cline CLI** (verified on 3.0.73-3.0.75), logged in with `cline auth`.
  Cline keeps its login in its config directory, so a Cline worker runs with
  your Cline state (plugins, MCP servers, hooks). `references/cline.md` says
  what to check first.
- `git`, `jq`, and standard coreutils.

## Free models

`SKILL.md` has a small benchmark of every free, available opencode model on
the author's machine (2026-11-22, one run per model, so a calibration, not a
ranking). It tested an implementation task scored by 27 hidden cases and a
review task with 3 planted defects.

- **Default:** `opencode/deepseek-v4.1-flash-free` (27/27 in 9s).
- **Second reviewer:** `opencode/big-pickle`, the only model that found all
  three defects with no debatable finding and also passed the implementation
  task.
- **Cline:** the default is the first id on its `.free` list
  (`cline-free/nemotron-3.5-super-vl` that day).

Free pools have limits, and they run out:

| Pool | Limit seen | Error |
|---|---|---|
| OpenCode Zen (`opencode/*-free`) | one quota for every free model of the account; one ~23-minute review turn used it up | `429` `provider.free-limit` |
| OpenRouter (`openrouter/...:free`) | [20 requests/minute](https://openrouter.ai/docs/api/reference/limits) (a short wait clears it) and 50 requests/day under 10 credits bought, 1,000 from 10 (lasts the day); each agent step is one request, so one multi-step turn used up the 50 | `429` (per minute, transient) / `429` `free-models-per-day` (daily) |
| NVIDIA (`nvidia/...`) | separate pool, still answering when the two above were spent | -- |
| Cline (`cline-free/...`) | free promotions end: 5 of 6 ids ended on one day | `Free model promotion ended` |

So check the pools before a large fan-out, spread workers over them, and keep
each turn bounded. A `410`/`404` (model retired) or a quota `429` means
switching model or pool, not resending.

## Usage examples

```
Delegate implementing the CSV export function to opencode.

Use Cline's free model to write tests for the parser module.

Fan out 3 independent free workers to port the utils/ files to TypeScript.

Have a free model from another family review this diff, then verify its findings.
```

Full behavior, all flags, safety rules, and known operating caveats are
documented in [`SKILL.md`](SKILL.md) -- read it before relying on this for
anything important.

## In real use

The playbook was used on a real project (decaf, a Java bytecode runtime in
Rust plus Python tooling). There, free workers:

- **Wrote a portability fix in three turns.** The fix replaced a Python
  3.14-only helper with arithmetic that matches 3.11 to 3.14. The third turn
  corrected a wrong rule in the orchestrator's own prompt, which only
  running the tests under each Python version exposed.
- **Drafted a documentation fix.** It worked on the second pool, after the
  first one's quota ran out.
- **Gave a cross-model review.** It found one real docstring
  overstatement that the same-family reviewers had missed.

Every result was reviewed and re-run by the orchestrator before it was kept.
Each such use also found something to fix in the playbook, and the git log
records each fix with what was observed.

## Platform support

Written and tested on Linux (Arch). The playbook is a bash script embedded in
markdown -- `mktemp`, `awk`, `jq`, `timeout`, heredocs, `$(...)` -- so it needs
a real POSIX-ish shell.

- **Linux / macOS:** works as documented (macOS untested by the author).
- **Windows via WSL:** use `SKILL.md` as-is inside a WSL distro (a real Linux
  userspace) -- untested by the author (no Windows machine available), but
  nothing in the playbook is Linux-specific once you're inside WSL. The worker
  CLI and `jq` need to be installed *inside* the distro, not on the Windows
  side.
- **Native Windows (PowerShell 7, no WSL):**
  [`references/windows.md`](references/windows.md) translates the **2.x** playbook
  (isolated config, closed stdin, `--standalone`, the supervisor, session ids from the
  event stream, free checks, cleanup), with no `jq` dependency. It is **untested on a
  real Windows machine**: issues/PRs from anyone who can run it there are welcome.

## Token savings (rough estimate)

**This is reasoning about the mechanics, not a measured benchmark** -- no A/B
test exists comparing "the orchestrating model does the task itself" against
"it delegates" on the same task. Treat the numbers below as directional, not
precise.

The core idea: when you delegate, the orchestrating model only pays for
scoping the task and reviewing the result -- the delegated model absorbs the
expensive part (reading context, reasoning through the implementation,
generating the code) at whatever the delegated model's own cost is (with a
free model, $0). The more expensive the orchestrating model's own
reasoning would have been, the more there is to save by not spending it.

Rough range for a medium, well-scoped task (a function plus tests, clear
spec), **in the success case**:

| Orchestrating model / effort | Est. tokens if done directly | Est. tokens if delegated + reviewed | Est. savings |
|---|---|---|---|
| Frontier model, max reasoning effort | 15,000-40,000+ | 3,000-8,000 | ~70-85% |
| Frontier model, default reasoning | 6,000-15,000 | 2,500-6,000 | ~50-65% |
| Mid-size model, default | 3,000-8,000 | 2,000-4,000 | ~30-45% |
| Small/fast model, low effort | 1,500-4,000 | 1,200-2,500 | ~15-30% |

**The success case is doing a lot of work in that table.** On opencode 1.x,
during this project's early development, 2 of 9 real tool-use delegation
attempts completed before a timeout. Some of those failures may have been the stdin
hang described below, which the playbook now avoids, but a failed attempt
still costs the scoping prompt and the wait. So expected savings are lower
than the table, and can go negative for small tasks delegated speculatively.
The "Is this task a fit for delegation?" section in `SKILL.md` exists to keep
you on the winning side of that math -- use it as a real filter.

## Known limitations

- **Bootstrap hang (opencode 2.1.4).** `opencode run` reads stdin when it
  isn't a terminal and waits for it to close. Under an agent's shell tool,
  stdin is usually an open pipe or socket that never closes. Same command,
  same machine: inherited stdin hung 2/2 until the timeout, while `</dev/null`
  finished 2/2 in about two seconds. On 2.3.2 an inherited stdin didn't hang
  (1/1). The playbook always closes stdin.
- **Mid-turn stalls.** Free and shared backends can still stall mid-turn,
  sometimes after the target file was already edited correctly on disk. So
  every turn is **supervised** rather than run under a fixed timeout: it
  waits while the worker's own event stream and server log are moving, kills
  it once they go quiet, and inspects the worktree before concluding nothing
  happened (step 2 of `SKILL.md`).
- **Cline workers aren't config-isolated.** Isolating them would drop the
  login, so they inherit your Cline plugins, MCP servers and hooks.
- **Cline can't continue a session non-interactively.** A refinement is a
  fresh turn whose prompt carries the context.
- **Free quotas are small.** See the table above.

## License

[GPL-3.0](LICENSE).
