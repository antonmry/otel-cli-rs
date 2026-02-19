#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "datafusion>=43.0",
# ]
# ///
"""Query OTel parquet data from S3 using DataFusion."""

from datafusion import SessionContext
from datafusion.object_store import AmazonS3

import os

BUCKET = os.environ["OTEL_S3_BUCKET"]
REGION = os.environ.get("OTEL_S3_REGION", "eu-west-1")

ctx = SessionContext()

# Configure S3 with default AWS credentials
s3 = AmazonS3(
    bucket_name=BUCKET,
    region=REGION,
)
ctx.register_object_store(f"s3://{BUCKET}/", s3)

# Discover available tables by listing known paths
TABLES = ["logs", "log_attrs", "resource_attrs",
           "spans", "span_attrs", "span_events", "span_links",
           "metrics", "metric_attrs"]

registered = []
for table in TABLES:
    try:
        ctx.register_parquet(table, f"s3://{BUCKET}/{table}/")
        registered.append(table)
    except Exception:
        pass

print(f"Registered tables: {', '.join(registered)}\n")

# Show schema for each table
for table in registered:
    print("=" * 70)
    print(f"SCHEMA — {table}")
    print("=" * 70)
    df = ctx.sql(f"SELECT * FROM {table} LIMIT 0")
    print(df.schema())
    print()

# Show row counts
print("=" * 70)
print("ROW COUNTS")
print("=" * 70)
for table in registered:
    df = ctx.sql(f"SELECT COUNT(*) AS cnt FROM {table}")
    result = df.collect()
    count = result[0].column("cnt")[0].as_py()
    print(f"  {table:20s} {count:>8,} rows")

# Show sample rows for each table
for table in registered:
    print()
    print("=" * 70)
    print(f"SAMPLE — {table} (10 rows)")
    print("=" * 70)
    try:
        df = ctx.sql(f"SELECT * FROM {table} LIMIT 10")
        df.show()
    except Exception as e:
        print(f"  (SELECT * failed: {e})")
        print("  Trying COUNT(*) only...")
        df = ctx.sql(f"SELECT COUNT(*) AS cnt FROM {table}")
        df.show()

print("\nDone.")
