
$GOPATH/bin/telemetrygen logs --duration 5s --otlp-insecure

$GOPATH/bin/telemetrygen metrics --otlp-insecure --metrics 1

$GOPATH/bin/telemetrygen metrics --otlp-insecure --otlp-endpoint \
127.0.0.1:10000 --metrics 1
