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

Claude Code picks it up automatically -- ask it to delegate something to
opencode, or run `/delegate-to-opencode`.

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

## Usage examples

```
Delegate implementing the CSV export function to opencode.

Use opencode with claude-3-5-haiku to write tests for the parser module.

Fan out 3 independent opencode workers to port the utils/ files to TypeScript.
```

Full behavior, all flags, safety rules, and known operating caveats are
documented in [`SKILL.md`](SKILL.md) -- read it before relying on this for
anything important.

## Known limitation

`opencode run` has been observed to hang indefinitely on tool-use turns, with
no error and no exit, on at least one setup -- root cause wasn't pinned down.
The playbook wraps every invocation in `timeout` and checks the filesystem
before assuming a hang means the work didn't happen (it sometimes already
had). See the "timeout is not optional" note in `SKILL.md` for details. Issues
and PRs narrowing this down further are welcome.

## License

[GPL-3.0](LICENSE).
