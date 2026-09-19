# delegate-to-opencode

An agent playbook for delegating implementation work to a model via the
[`opencode`](https://opencode.ai) CLI, while the orchestrating agent stays in
the planner/reviewer role: it scopes the task, writes the prompt, and reviews
and iterates on the worker's output before accepting anything -- never
trusting `opencode`'s exit code or the model's own claims of success without
independently checking the diff and running real verification.

Defaults to the free `opencode/nemotron-3-ultra-free` model ($0 cost), but any
model your `opencode` install supports can be used instead -- just say which
one you want.

Also supports fanning out several isolated `opencode` worker processes in
parallel for independent subtasks, each in its own git worktree or scratch
directory with a fresh session and no shared state, with the orchestrating
agent as the sole coordinator.

## What this actually is

All of the real content is [`SKILL.md`](SKILL.md). It ships with Claude Code
skill frontmatter (so Claude Code auto-discovers and triggers it), but the
body is plain, agent-agnostic instructions -- any coding agent that can run
bash and follow a written playbook (Codex, aider, a human at a terminal, ...)
can use it the same way. Just hand it the file.

## Install

**Claude Code:**

```bash
git clone https://github.com/Gabbotampo/delegate-to-opencode.git
ln -s "$(pwd)/delegate-to-opencode" ~/.claude/skills/delegate-to-opencode
```

Clone it wherever you keep your repos -- the symlink target is what matters,
not the clone location. Claude Code then picks it up automatically -- ask it
to delegate something to opencode, or run `/delegate-to-opencode`.

**Anything else (Codex, aider, a plain terminal session, ...):**

Point your agent at `SKILL.md` (or paste its contents into context) and treat
it as your instructions. The frontmatter block at the top is Claude-Code-only
metadata; skip straight to the `# Delegate to opencode` heading.

## Prerequisites

- `opencode` CLI installed and authenticated (`opencode auth login`).
- `opencode.json`'s `permission` block allowing non-interactive `read`/`edit`/`bash`
  (otherwise it will try to prompt for confirmation and hang -- see `SKILL.md`
  for why, and why `--dir` matters).
- `git`, `jq`, and standard coreutils.

## Platform support

Written and tested on Linux (Arch). The playbook is a bash script embedded in
markdown -- `mktemp`, `awk`, `jq`, `timeout`, heredocs, `$(...)` -- so it needs
a real POSIX-ish shell.

- **Linux / macOS:** works as documented.
- **Windows via WSL:** use `SKILL.md` as-is inside a WSL distro (a real Linux
  userspace) -- untested by the author (no Windows machine available), but
  there's nothing in the playbook that's Linux-specific once you're inside
  WSL, beyond `opencode` and `jq` needing to be installed *inside* the distro,
  not the Windows side.
- **Native Windows (PowerShell, no WSL):** see
  [`references/windows.md`](references/windows.md) -- a full PowerShell
  translation of every command block, same rules and reasoning as
  `SKILL.md`. **Also untested on a real Windows machine** -- it's a careful
  translation, not a verified port. Issues/PRs from anyone who can actually
  run it on Windows are genuinely welcome.

## Usage examples

```
Delegate implementing the CSV export function to opencode.

Use opencode with claude-3-5-haiku to write tests for the parser module.

Fan out 3 independent opencode workers to port the utils/ files to TypeScript.
```

Full behavior, all flags, safety rules, and known operating caveats are
documented in [`SKILL.md`](SKILL.md) -- read it before relying on this for
anything important.

## Token savings (rough estimate)

**This is reasoning about the mechanics, not a measured benchmark** -- no A/B
test exists comparing "the orchestrating model does the task itself" against
"it delegates" on the same task. Treat the numbers below as directional, not
precise.

The core idea: when you delegate, the orchestrating model only pays for
scoping the task and reviewing the result -- the delegated model absorbs the
expensive part (reading context, reasoning through the implementation,
generating the code) at whatever the delegated model's own cost is (with the
default model, $0). The more expensive the orchestrating model's own
reasoning would have been, the more there is to save by not spending it.

Rough range for a medium, well-scoped task (a function plus tests, clear
spec), **in the success case**:

| Orchestrating model / effort | Est. tokens if done directly | Est. tokens if delegated + reviewed | Est. savings |
|---|---|---|---|
| Frontier model, max reasoning effort | 15,000-40,000+ | 3,000-8,000 | ~70-85% |
| Frontier model, default reasoning | 6,000-15,000 | 2,500-6,000 | ~50-65% |
| Mid-size model, default | 3,000-8,000 | 2,000-4,000 | ~30-45% |
| Small/fast model, low effort | 1,500-4,000 | 1,200-2,500 | ~15-30% |

**The success case is doing a lot of work in that table.** In real testing
against the free default model during this project's own development, 2 of 9
real tool-use delegation attempts actually completed before a timeout (see
Known limitation below). Every failed attempt still costs the scoping prompt
and the time spent waiting before falling back to doing the work directly --
so the *expected* savings, weighted by observed success rate, are
meaningfully lower than the table above, and can go negative for small tasks
delegated speculatively. The "Is this task a fit for delegation?" section in
`SKILL.md` exists specifically to keep you on the winning side of that
math -- it's not boilerplate, use it as a real filter.

## Known limitation

`opencode run` has been observed to hang indefinitely -- no output, no error,
no exit -- on at least one setup, in two distinct modes: during **bootstrap**
(before the session is even created, nothing done) and **mid-turn** (where the
target file had already been read *and correctly edited* on disk before the
process stalled on a later internal call). Root cause wasn't pinned down.

Rather than a fixed timeout, every delegation turn is **supervised**: the
playbook watches opencode's terminal event (`step_finish` with
`part.reason == "stop"`) for completion and its internal log for liveness --
so it keeps waiting while real progress is happening and kills only once
things go quiet. On any non-completion outcome it inspects the worktree before
concluding nothing happened, precisely because of the mid-turn case above. See
step 2 of `SKILL.md` for the full mechanism. Issues and PRs narrowing the root
cause down further are welcome.

## License

[GPL-3.0](LICENSE).
