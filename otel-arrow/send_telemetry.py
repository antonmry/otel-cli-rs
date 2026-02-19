#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "opentelemetry-api>=1.20",
#     "opentelemetry-sdk>=1.20",
#     "opentelemetry-exporter-otlp-proto-grpc>=1.20",
# ]
# ///
"""Send sample OTel telemetry (logs, traces, metrics) to an OTLP collector."""

import time

from opentelemetry import trace, metrics
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor
from opentelemetry.sdk.metrics import MeterProvider
from opentelemetry.sdk.metrics.export import PeriodicExportingMetricReader
from opentelemetry.sdk.resources import Resource
from opentelemetry.sdk._logs import LoggerProvider, LoggingHandler
from opentelemetry.sdk._logs.export import BatchLogRecordProcessor
from opentelemetry.exporter.otlp.proto.grpc.trace_exporter import OTLPSpanExporter
from opentelemetry.exporter.otlp.proto.grpc.metric_exporter import OTLPMetricExporter
from opentelemetry.exporter.otlp.proto.grpc._log_exporter import OTLPLogExporter

import logging

ENDPOINT = "127.0.0.1:5317"
NUM_ITERATIONS = 10
FLUSH_TIMEOUT_MS = 30000

resource = Resource.create({
    "service.name": "e2e-test-service",
    "service.version": "1.0.0",
    "deployment.environment": "testing",
})

# --- Traces ---
trace_exporter = OTLPSpanExporter(endpoint=ENDPOINT, insecure=True)
trace_provider = TracerProvider(resource=resource)
trace_provider.add_span_processor(BatchSpanProcessor(trace_exporter))
trace.set_tracer_provider(trace_provider)
tracer = trace.get_tracer("e2e-test", "1.0.0")

# --- Metrics ---
metric_exporter = OTLPMetricExporter(endpoint=ENDPOINT, insecure=True)
metric_reader = PeriodicExportingMetricReader(metric_exporter, export_interval_millis=5000)
meter_provider = MeterProvider(resource=resource, metric_readers=[metric_reader])
metrics.set_meter_provider(meter_provider)
meter = metrics.get_meter("e2e-test", "1.0.0")

request_counter = meter.create_counter(
    "http.server.request.count",
    description="Total HTTP requests",
    unit="1",
)
request_duration = meter.create_histogram(
    "http.server.request.duration",
    description="HTTP request duration",
    unit="ms",
)

# --- Logs ---
log_exporter = OTLPLogExporter(endpoint=ENDPOINT, insecure=True)
logger_provider = LoggerProvider(resource=resource)
logger_provider.add_log_record_processor(BatchLogRecordProcessor(log_exporter))
handler = LoggingHandler(level=logging.INFO, logger_provider=logger_provider)
logger = logging.getLogger("e2e-test")
logger.addHandler(handler)
logger.setLevel(logging.INFO)


def simulate_request(i: int) -> None:
    """Simulate a single request producing traces, metrics, and logs."""
    with tracer.start_as_current_span(
        "handle_request",
        attributes={
            "http.method": "GET",
            "http.route": "/api/items",
            "http.status_code": 200,
        },
    ) as span:
        # Nested span
        with tracer.start_as_current_span(
            "db_query",
            attributes={
                "db.system": "postgresql",
                "db.statement": "SELECT * FROM items",
            },
        ):
            time.sleep(0.01)  # Simulate DB work

        # Metrics
        request_counter.add(1, {"http.method": "GET", "http.route": "/api/items"})
        request_duration.record(
            15.0 + (i % 10),
            {"http.method": "GET", "http.route": "/api/items"},
        )

        # Log
        logger.info(
            "Handled request %d for /api/items",
            i,
            extra={"http.method": "GET", "request.id": f"req-{i:04d}"},
        )


print(f"Sending {NUM_ITERATIONS} simulated requests to {ENDPOINT}...")

for i in range(NUM_ITERATIONS):
    simulate_request(i)
    time.sleep(0.1)

print("Flushing telemetry...")
trace_provider.force_flush(timeout_millis=FLUSH_TIMEOUT_MS)
logger_provider.force_flush(timeout_millis=FLUSH_TIMEOUT_MS)
meter_provider.force_flush(timeout_millis=FLUSH_TIMEOUT_MS)

trace_provider.shutdown()
logger_provider.shutdown()
meter_provider.shutdown()

print(f"Done. Sent {NUM_ITERATIONS} traces, logs, and metric data points.")
