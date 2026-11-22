#!/usr/bin/env python3
"""cmp.py java25.txt rt.txt: per-row verdict, rows aligned by position (row i of the probe).
SILENT = runtime answered (no unknown host call) and differs. ERROR = the runtime failed the row."""
import sys
java = [l.rstrip('\n') for l in open(sys.argv[1], encoding='utf-8', errors='replace')]
rt = []
for l in open(sys.argv[2], encoding='utf-8', errors='replace'):
    val, _, unk = l.rstrip('\n').partition('\t')
    rt.append((val, unk))
counts = {'EXACT': 0, 'COUNTED': 0, 'SILENT': 0, 'ERROR': 0, 'MISSING': 0}
out = []
for i, j in enumerate(java):
    if i >= len(rt):
        counts['MISSING'] += 1; out.append(f'MISSING\t{j}\t| runtime stopped before this row'); continue
    v, unk = rt[i]
    if v.startswith('ERR '):
        counts['ERROR'] += 1; out.append(f'ERROR\t{j}\t| runtime: {v[:300]}\t| {unk}')
    elif unk:
        counts['COUNTED'] += 1; out.append(f'COUNTED\t{j}\t| {unk}')
    elif v == j:
        counts['EXACT'] += 1
    else:
        counts['SILENT'] += 1; out.append(f'SILENT\tjava: {j}\t| runtime: {v}')
order = ('SILENT', 'ERROR', 'COUNTED', 'MISSING')
print('\n'.join(sorted(out, key=lambda s: order.index(s.split('\t')[0]))))
print('TOTALS ' + ' '.join(f'{k}={v}' for k, v in counts.items()))
