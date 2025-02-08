use crate::error::DashboardError;
use opentelemetry_proto::tonic::collector::metrics::v1::{
    metrics_service_server::{MetricsService, MetricsServiceServer},
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use std::collections::HashSet;
use tokio::sync::{mpsc::UnboundedSender, Mutex as TokioMutex};
use tonic::{Request, Response, Status};

#[derive(Debug)]
pub enum UiMessage {
    NewMetric(String),
    MetricUpdate(String),
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

    pub async fn send_metric_update(&self, metric_name: &str, details: String) {
        if let Err(e) = self.ui_tx.send(UiMessage::MetricUpdate(format!(
            "{}: {}",
            metric_name, details
        ))) {
            eprintln!("Failed to send metric update: {}", e);
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
                        Some(data) => match data {
                            opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(gauge) => {
                                for point in &gauge.data_points {
                                    self.send_metric_update(
                                        &metric.name,
                                        format!("= {:?}", point.value),
                                    )
                                    .await;
                                }
                            }
                            opentelemetry_proto::tonic::metrics::v1::metric::Data::Sum(sum) => {
                                for point in &sum.data_points {
                                    self.send_metric_update(
                                        &metric.name,
                                        format!("= {:?}", point.value),
                                    )
                                    .await;
                                }
                            }
                            opentelemetry_proto::tonic::metrics::v1::metric::Data::Histogram(
                                hist,
                            ) => {
                                for point in &hist.data_points {
                                    self.send_metric_update(
                                        &metric.name,
                                        format!("count: {}, sum: {:?}", point.count, point.sum),
                                    )
                                    .await;
                                }
                            }
                            _ => {}
                        },
                        None => {}
                    }
                }
            }
        }

        Ok(Response::new(ExportMetricsServiceResponse::default()))
    }
}

pub fn create_metrics_service(
    debug_mode: bool,
    ui_tx: UnboundedSender<UiMessage>,
) -> MetricsServiceServer<MetricsReceiver> {
    MetricsServiceServer::new(MetricsReceiver::new(debug_mode, ui_tx))
}
