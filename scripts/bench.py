#!/usr/bin/env python3
"""aider-rs performance acceptance bench (ticket 06).

Measures the three T06 acceptance numbers against the release binary:
  - cold start (spawn -> first initialize response)   target < 200ms
  - resident RSS after serving one call               target < 50MB
  - binary size                                       target < 15MB

Usage: python3 scripts/bench.py [path-to-release-binary]
"""

import json
import os
import subprocess
import sys
import time

BIN = sys.argv[1] if len(sys.argv) > 1 else "plugin/bin/aider-rs"
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

if not os.path.exists(BIN):
    sys.exit(f"binary not found: {BIN} (run ./plugin/install.sh first)")

size_mb = os.path.getsize(BIN) / 1024 / 1024

env = dict(os.environ, OPENAI_API_KEY="bench", AIDER_RS_MODEL="bench-model")
p = subprocess.Popen(
    [BIN], cwd=REPO, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
    stderr=subprocess.DEVNULL, env=env, text=True,
)

t0 = time.perf_counter()
p.stdin.write(json.dumps({
    "jsonrpc": "2.0", "id": 1, "method": "initialize",
    "params": {"protocolVersion": "2024-11-05", "capabilities": {},
               "clientInfo": {"name": "bench", "version": "0"}},
}) + "\n")
p.stdin.flush()
line = p.stdout.readline()
cold_ms = (time.perf_counter() - t0) * 1000
assert json.loads(line)["result"]["serverInfo"]["name"] == "aider-rs"

t0 = time.perf_counter()
p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}) + "\n")
p.stdin.flush()
p.stdout.readline()
warm_ms = (time.perf_counter() - t0) * 1000

with open(f"/proc/{p.pid}/status") as f:
    rss_mb = next(int(l.split()[1]) for l in f if l.startswith("VmRSS")) / 1024

p.stdin.close()
p.wait(timeout=5)

print(f"binary_mb:    {size_mb:.1f}  (target < 15)")
print(f"cold_start_ms: {cold_ms:.1f}  (target < 200)")
print(f"warm_dispatch_ms: {warm_ms:.2f}")
print(f"rss_mb:       {rss_mb:.1f}  (target < 50)")
ok = size_mb < 15 and cold_ms < 200 and rss_mb < 50
print("VERDICT:", "PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
