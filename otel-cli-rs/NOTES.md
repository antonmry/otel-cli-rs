
$GOPATH/bin/telemetrygen logs --duration 5s --otlp-insecure

$GOPATH/bin/telemetrygen metrics --otlp-insecure --metrics 1

$GOPATH/bin/telemetrygen metrics --otlp-insecure --otlp-endpoint \
127.0.0.1:10000 --metrics 1


$GOPATH/bin/telemetrygen metrics --otlp-endpoint=http://localhost:4317 \
  --telemetry-attributes metric.name="cpu.usage" metric.type="system"

$GOPATH/bin/telemetrygen metrics --otlp-insecure --telemetry-attributes name=\"cpu\" type=\"system\"

$GOPATH/bin/telemetrygen metrics --otlp-insecure --metric-names "cpu.usage,memory.used,disk.io"

$GOPATH/bin/telemetrygen metrics --otlp-attributes metric.description=\"test\" --telemetry-attributes metric.name=\"frontend2\" --otlp-insecure

