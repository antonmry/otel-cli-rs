#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""Generate collector config files by replacing OTEL_S3_BUCKET placeholder."""

import os
import sys
from pathlib import Path

PLACEHOLDER = "OTEL_S3_BUCKET"

bucket = os.environ.get("OTEL_S3_BUCKET")
if not bucket:
    print(f"Error: OTEL_S3_BUCKET environment variable is not set.", file=sys.stderr)
    sys.exit(1)

templates = list(Path(__file__).parent.glob("*.yaml"))
if not templates:
    print("No .yaml templates found.", file=sys.stderr)
    sys.exit(1)

for template in templates:
    content = template.read_text()
    if PLACEHOLDER not in content:
        continue
    out = template.with_suffix(".gen.yaml")
    out.write_text(content.replace(PLACEHOLDER, bucket))
    print(f"  {template.name} -> {out.name}")

print("Done.")
