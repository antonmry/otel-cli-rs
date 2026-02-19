# E2E Test: OTLP → Parquet → S3

End-to-end test of the S3 parquet exporter using a real AWS account with
the OTel Python SDK as the telemetry source and DataFusion for querying results.

## Prerequisites

- Rust >= 1.87.0
- [uv](https://github.com/astral-sh/uv) (Python script runner)
- AWS CLI v2
- An AWS account with S3 access

## Step 1: Configure Environment

Export your credentials, region, and S3 bucket name:

```bash
export AWS_ACCESS_KEY_ID=your-access-key
export AWS_SECRET_ACCESS_KEY=your-secret-key
export AWS_DEFAULT_REGION=eu-west-1
export OTEL_S3_BUCKET=your-bucket-name
```

Another option:

```bash
eval "$(aws configure export-credentials --profile xxx --format env)"
export OTEL_S3_BUCKET=your-bucket-name
```


## Step 2: Create the S3 Bucket

```bash
aws s3 mb s3://$OTEL_S3_BUCKET
```

Verify:

```bash
aws s3 ls | grep $OTEL_S3_BUCKET
```

## Step 3: Build the Collector

```bash
cd otel-arrow/rust/otap-dataflow
cargo install --features aws --path .
```

## Step 4: Generate Config Files

The YAML templates use `OTEL_S3_BUCKET` as a placeholder. Generate the
actual config files with your bucket name:

```bash
uv run gen_config.py
```

This reads all `*.yaml` files, replaces `OTEL_S3_BUCKET` with the value
from your environment, and writes `*.gen.yaml` files. The generated files
are gitignored.

## Step 5: Start the Collector

In a dedicated terminal (with the env vars exported):

```bash
df_engine -c otlp-parquet-s3.gen.yaml
```

This starts an OTLP receiver on:

- gRPC: `0.0.0.0:5317`
- HTTP: `0.0.0.0:5318`

And exports parquet files to `s3://$OTEL_S3_BUCKET/test`.

To auto-generate telemetry without an external sender:

```bash
df_engine -c fake-parquet-s3.gen.yaml
```

## Step 6: Send Telemetry

In a second terminal:

```bash
uv run send_telemetry.py
```

This sends 10 simulated requests, each producing:

- **Traces** — `handle_request` span with nested `db_query` child span
- **Metrics** — `http.server.request.count` counter and
  `http.server.request.duration` histogram
- **Logs** — structured log records with request context

## Step 7: Wait for Flush

The parquet exporter is configured with `flush_when_older_than: 10s`.
Wait at least **15 seconds** after the script finishes for the data to be
flushed to S3.

## Step 8: Verify Files in S3

```bash
aws s3 ls s3://$OTEL_S3_BUCKET/ --recursive
```

You should see `.parquet` files under directories like `logs/`, `log_attrs/`,
`resource_attrs/`, and potentially `spans/`, `metrics/`, etc.

## Step 9: Query with DataFusion

```bash
uv run query_s3.py
```

The script reads `OTEL_S3_BUCKET` from the environment to locate the bucket.

## Step 10: Run Unit Tests

```bash
cd rust/otap-dataflow
cargo test --features aws -p otap-df-otap
```

## Cleanup

```bash
# Empty and delete the bucket
aws s3 rm s3://$OTEL_S3_BUCKET/ --recursive
aws s3 rb s3://$OTEL_S3_BUCKET

# Stop the collector with Ctrl+C
```

## Troubleshooting

| Symptom                                | Fix                                                                                                                                                                                                     |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Collector fails with credentials error | Verify `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, and `AWS_DEFAULT_REGION` are exported in the terminal running the collector                                                                        |
| `send_telemetry.py` fails to connect   | Ensure the collector is running and listening on port 5317                                                                                                                                              |
| No parquet files after 15s             | Check collector logs for S3 write errors. Common issues: wrong region, insufficient IAM permissions (`s3:PutObject`, `s3:GetObject`, `s3:ListBucket`)                                                   |
| `query_s3.py` finds no files           | Ensure `OTEL_S3_BUCKET` is set. List files with `aws s3 ls --recursive` first. Check that parquet files exist in subdirectories (`logs/`, `log_attrs/`, `resource_attrs/`)                              |
| Permission denied on S3                | The IAM user/role needs at minimum: `s3:PutObject`, `s3:GetObject`, `s3:ListBucket` on the bucket                                                                                                       |

## Files Reference

| File                   | Purpose                                                     |
| ---------------------- | ----------------------------------------------------------- |
| `otlp-parquet-s3.yaml` | Template: OTLP receiver → S3 parquet exporter               |
| `fake-parquet-s3.yaml` | Template: traffic generator → S3 parquet exporter           |
| `fake-parquet.yaml`    | Template: traffic generator → local parquet exporter        |
| `gen_config.py`        | Generates `*.gen.yaml` from templates with your bucket name |
| `send_telemetry.py`    | Sends traces, metrics, and logs via OTLP gRPC               |
| `query_s3.py`          | Queries parquet files from S3 using DataFusion              |
