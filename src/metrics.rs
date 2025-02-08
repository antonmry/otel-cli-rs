use opentelemetry_proto::tonic::collector::metrics::v1::{
    metrics_service_server::{MetricsService, MetricsServiceServer},
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use tokio::sync::{mpsc::UnboundedSender, Mutex as TokioMutex};
use tonic::{Request, Response, Status};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct MetricPoint {
    pub timestamp: u64,
    pub value: f64,
}

#[derive(Debug)]
pub enum UiMessage {
    NewMetric(String),
    MetricUpdate(String),
    MetricDataPoint { 
        name: String, 
        point: MetricPoint 
    },
}

pub struct MetricsReceiver {
    seen_metrics: TokioMutex<HashSet<String>>,
    debug_mode: bool,
    ui_tx: UnboundedSender<UiMessage>,
}

impl MetricsReceiver {
    pub fn new(debug_mode: bool, ui_tx: UnboundedSender<UiMessage>) -> Self {
        Self {
            seen_metrics: TokioMutex::new(HashSet::new()),
            debug_mode,
            ui_tx,
        }
    }

    fn get_current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    async fn send_metric_update(&self, metric_name: &str, details: String) {
        if let Err(e) = self.ui_tx.send(UiMessage::MetricUpdate(
            format!("{}: {}", metric_name, details)
        )) {
            eprintln!("Failed to send metric update: {}", e);
        }
    }

    async fn send_metric_datapoint(&self, name: String, value: f64) {
        let point = MetricPoint {
            timestamp: Self::get_current_timestamp(),
            value,
        };

        if let Err(e) = self.ui_tx.send(UiMessage::MetricDataPoint { 
            name, 
            point,
        }) {
            eprintln!("Failed to send metric datapoint: {}", e);
        }
    }

    fn extract_value(value: &opentelemetry_proto::tonic::metrics::v1::number_data_point::Value) -> Option<f64> {
        match value {
            opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(v) => Some(*v),
            opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(v) => Some(*v as f64),
        }
    }
}

#[tonic::async_trait]
impl MetricsService for MetricsReceiver {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        let metrics = request.into_inner();
        let mut seen_metrics = self.seen_metrics.lock().await;
        
        for resource_metrics in metrics.resource_metrics {
            for scope_metrics in &resource_metrics.scope_metrics {
                for metric in &scope_metrics.metrics {
                    if seen_metrics.insert(metric.name.clone()) {
                        if let Err(e) = self.ui_tx.send(UiMessage::NewMetric(metric.name.clone())) {
                            eprintln!("Failed to send new metric: {}", e);
                        }
                    }
                    
                    match &metric.data {
                        Some(data) => {
                            match data {
                                opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(gauge) => {
                                    for point in &gauge.data_points {
                                        if let Some(value) = point.value.as_ref().and_then(Self::extract_value) {
                                            self.send_metric_datapoint(metric.name.clone(), value).await;
                                        }
                                        self.send_metric_update(&metric.name, 
                                            format!("= {:?}", point.value)
                                        ).await;
                                    }
                                },
                                opentelemetry_proto::tonic::metrics::v1::metric::Data::Sum(sum) => {
                                    for point in &sum.data_points {
                                        if let Some(value) = point.value.as_ref().and_then(Self::extract_value) {
                                            self.send_metric_datapoint(metric.name.clone(), value).await;
                                        }
                                        self.send_metric_update(&metric.name, 
                                            format!("= {:?}", point.value)
                                        ).await;
                                    }
                                },
                                opentelemetry_proto::tonic::metrics::v1::metric::Data::Histogram(hist) => {
                                    for point in &hist.data_points {
                                        if let Some(sum) = point.sum {
                                            self.send_metric_datapoint(metric.name.clone(), sum).await;
                                        }
                                        self.send_metric_update(&metric.name, 
                                            format!("count: {}, sum: {:?}", point.count, point.sum)
                                        ).await;
                                    }
                                },
                                _ => {}
                            }
                        },
                        None => {}
                    }
                }
            }
        }

        Ok(Response::new(ExportMetricsServiceResponse::default()))
    }
}

pub fn create_metrics_service(debug_mode: bool, ui_tx: UnboundedSender<UiMessage>) -> MetricsServiceServer<MetricsReceiver> {
    MetricsServiceServer::new(MetricsReceiver::new(debug_mode, ui_tx))
}
