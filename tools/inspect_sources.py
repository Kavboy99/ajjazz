"""Read source text without importing or executing vendor code."""
import argparse
import re
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

parser = argparse.ArgumentParser()
parser.add_argument("file", type=Path)
parser.add_argument("patterns", nargs="+")
parser.add_argument("--context", type=int, default=180)
parser.add_argument("--limit", type=int, default=15)
args = parser.parse_args()
source = args.file.read_text(encoding="utf-8", errors="replace")
for pattern in args.patterns:
    print(f"\nPATTERN: {pattern}")
    for match in list(re.finditer(pattern, source, re.IGNORECASE))[: args.limit]:
        start, end = max(0, match.start() - args.context), min(match.end(), match.start()+120) + args.context
        print(f"@{match.start()}: {source[start:end]}")
