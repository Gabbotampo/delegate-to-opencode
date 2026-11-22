# Differential hunt: let free models find bugs they cannot fake

When a program re-implements a reference (here: [decaf](https://github.com/Solenopsisbot/decaf),
a JVM-free Java interpreter that must answer JDK calls exactly as Java 25 does), cheap workers are
good at one thing: writing many probe rows. They are bad at reporting results honestly. So the
worker only writes the probe; scripts it cannot edit run it on the reference and on the program,
and compare.

## Flow

1. Each worker gets one API area (`hunter-prompt.md`, last line) and an empty directory.
2. It writes `demo/Probe.java` with `public static String row(int i)` returning one
   `label=value` line per case (exceptions printed as `EXC <class>: <message>`), `"NOROW"` after
   the last row.
3. It runs `run.sh`, which:
   - compiles the probe with `HuntRunner.java` and runs it on the real JDK, giving `java25.txt`.
     HuntRunner prints the rows and escapes them, so the worker cannot misalign or edit the
     output;
   - runs every row through the program under test with a prebuilt harness, giving `rt.txt`
     (each row's value plus the list of calls the program declined to answer);
   - compares the two row by row with `cmp.py`, giving `compare.txt`.

   Each row's verdict is one of:

   | Verdict | Meaning |
   |---|---|
   | `SILENT` | The program answered and differs. This is what we hunt. |
   | `COUNTED` | The program honestly declined. |
   | `ERROR` | The program failed the row. |
   | `EXACT` | The answer matches the reference. |
4. The worker adds rows around every `SILENT` one for 3 to 6 runs, then writes `REPORT.md`.
5. You (the coordinator) re-run `run.sh` yourself and read `compare.txt`, not the report. Each
   distinct bug then becomes a small, well-specified fix task: the evidence rows go into its prompt.

## Files

| File | |
|---|---|
| `hunter-prompt.md` | Worker prompt; replace the last line with the area. |
| `run.sh` | `HUNT_HARNESS=<harness binary> bash run.sh` from the worker's directory. |
| `HuntRunner.java` | Reference-side runner and escaping. |
| `cmp.py` | Row-by-row verdicts and totals. |
| `decaf-harness/` | Program side for decaf: a test binary that loads the probe source at run time, runs `row(i)` in a fresh host per row and prints the value plus decaf's unknown host calls. For another program, write the same thing: same escaping as `HuntRunner.esc`, one line per row. |

## Results (2026-11-22, 10 free NVIDIA workers, about an hour)

On decaf's main branch, they found:
- `ArrayDeque` accepting `null`;
- `Arrays.copyOfRange` with `from > to` returning `[]`;
- wrong NPE messages from `Objects.requireNonNullElse` and `TreeMap` null keys;
- `equalsIgnoreCase` false on non-ASCII text that Java folds.

Areas with no silent row (`Integer`/`Long` parsing, `String.format`, `StringBuilder`) are useful
evidence too.

## Pitfalls we hit

- **Long replies get truncated.** Free models trying to write a 600-row probe in one reply fail
  with `Response still truncated after two output token limit continuations`. Tell them to write
  it in steps.
- **Method size limits of the program under test.** decaf reads at most 256 exception handlers
  per method: tell workers to split rows over helper methods.
- **Unescaped output breaks the comparison.** One newline inside a value shifts every later row.
  That is why the runner, not the worker, prints and escapes.
