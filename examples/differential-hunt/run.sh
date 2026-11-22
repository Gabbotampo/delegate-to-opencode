#!/bin/bash
# run.sh : run the probe in the current directory (demo/Probe.java) on the real JDK and through the
# runtime under test, then compare row by row. Writes java25.txt (JDK output), rt.txt (runtime rows
# + its "unknown call" list) and compare.txt (one verdict per row, then TOTALS). Workers run this;
# they never write those files themselves, so they cannot invent results.
#
#   HUNT_HARNESS  the prebuilt runtime harness binary (see decaf-harness/)
#   HUNT_HOME     directory holding HuntRunner.java and cmp.py (default: this script's directory)
#   HUNT_TMP      scratch root (default /dev/shm/hunt-tmp)
set -u
D=$(pwd); HOME_DIR=${HUNT_HOME:-$(cd "$(dirname "$0")" && pwd)}
: "${HUNT_HARNESS:?set HUNT_HARNESS to the harness binary}"
T=${HUNT_TMP:-/dev/shm/hunt-tmp}/$(basename "$D"); rm -rf "$T"; mkdir -p "$T/classes"
javac --release 21 -d "$T/classes" demo/Probe.java "$HOME_DIR/HuntRunner.java" 2> javac.err || { echo "javac failed:"; cat javac.err; exit 1; }
timeout 300 java -Xmx384m -cp "$T/classes" HuntRunner 2>java.err | head -c 4000000 > java25.txt
echo "java rows: $(wc -l < java25.txt)"
TMPDIR="$T" PROBE="$D" OUT="$D/rt.txt" timeout 1200 "$HUNT_HARNESS" --quiet > rt.log 2>&1 || { echo "runtime harness failed:"; tail -20 rt.log; }
python3 "$HOME_DIR/cmp.py" java25.txt rt.txt > compare.txt
rm -rf "$T"
tail -5 compare.txt
