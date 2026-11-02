#!/usr/bin/env python3
"""Markdown table of a `scripts/propeller-suite.sh` run (or the second half of verify-all.sh).

usage: python3 scripts/suite_table.py <suite-output.txt>
One row per problem: runs compared, mismatches (sin/cos/tan last-digit cases counted apart),
oracle and Rust speed in million EXEC steps per second on the same programs.
"""
import re
import sys

rows, cur = [], None
for line in open(sys.argv[1], errors="replace"):
    m = re.match(r"== (\S+)", line)
    if m:
        cur = {"problem": m.group(1)}
        rows.append(cur)
        continue
    if cur is None:
        continue
    m = re.search(r"oracle execute only [\d.]+ s = \d+ runs/s, ([\d.]+) M steps/s", line)
    if m:
        cur["oracle"] = float(m.group(1))
    m = re.match(r"(\d+) cases, (\d+) mismatches, (\d+) further", line)
    if m:
        cur["cases"], cur["mism"], cur["trig"] = int(m.group(1)), int(m.group(2)), int(m.group(3))
    m = re.search(r"([\d.]+) M steps/s$", line.strip())
    if m and "oracle" not in line:
        cur["rust"] = float(m.group(1))

print("| Problem | Runs compared | Mismatches | Oracle M steps/s | Rust M steps/s |")
print("|---|---:|---:|---:|---:|")
tot = [0, 0]
for r in rows:
    if "cases" not in r:
        continue
    tot[0] += r["cases"]
    tot[1] += r["mism"]
    print(f"| {r['problem']} | {r['cases']:,} | {r['mism']} | {r.get('oracle', float('nan')):.2f} | {r.get('rust', float('nan')):.1f} |")
print(f"| **{len(rows)} problems** | **{tot[0]:,}** | **{tot[1]}** | | |")
