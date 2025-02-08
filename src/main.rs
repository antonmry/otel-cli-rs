use clap::Parser;
use std::{collections::HashSet, net::SocketAddr, sync::Mutex};
use thiserror::Error;
use tonic::{transport::Server, Request, Response, Status};
use opentelemetry_proto::tonic::collector::metrics::v1::{
    metrics_service_server::{MetricsService, MetricsServiceServer},
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};

#[derive(Error, Debug)]
pub enum DashboardError {
    #[error("Failed to start server: {0}")]
    ServerError(#[from] tonic::transport::Error),
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "127.0.0.1:4317")]
    address: SocketAddr,

    #[arg(short, long)]
    debug: bool,
}

#[derive(Debug)]
struct MetricsReceiver {
    seen_metrics: Mutex<HashSet<String>>,
    debug_mode: bool,
}

impl MetricsReceiver {
    fn new(debug_mode: bool) -> Self {
        Self {
            seen_metrics: Mutex::new(HashSet::new()),
            debug_mode,
        }
    }

    fn print_debug_metric(&self, resource_metrics: &opentelemetry_proto::tonic::metrics::v1::ResourceMetrics, 
                         scope_metrics: &opentelemetry_proto::tonic::metrics::v1::ScopeMetrics, 
                         metric: &opentelemetry_proto::tonic::metrics::v1::Metric) {
        println!("\n=== Detailed Metric Information ===");
        
        // Print Resource information
        if let Some(resource) = &resource_metrics.resource {
            println!("Resource Attributes:");
            for attr in &resource.attributes {
                if let Some(value) = &attr.value {
                    println!("\t{} = {:?}", attr.key, value.value);
                }
            }
        }

        // Print Scope information
        if let Some(scope) = &scope_metrics.scope {
            println!("\nInstrumentation Scope:");
            println!("\tName: {}", scope.name);
            println!("\tVersion: {}", scope.version);
            if !scope.attributes.is_empty() {
                println!("\tAttributes:");
                for attr in &scope.attributes {
                    if let Some(value) = &attr.value {
                        println!("\t\t{} = {:?}", attr.key, value.value);
                    }
                }
            }
        }

        // Print Metric details
        println!("\nMetric Details:");
        println!("\tName: {}", metric.name);
        println!("\tDescription: {}", metric.description);
        println!("\tUnit: {}", metric.unit);

        match &metric.data {
            Some(data) => {
                match data {
                    opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(gauge) => {
                        println!("\tType: Gauge");
                        for point in &gauge.data_points {
                            println!("\tDataPoint:");
                            println!("\t\tValue: {:?}", point.value);
                            if !point.attributes.is_empty() {
                                println!("\t\tAttributes:");
                                for attr in &point.attributes {
                                    if let Some(value) = &attr.value {
                                        println!("\t\t\t{} = {:?}", attr.key, value.value);
                                    }
                                }
                            }
                            println!("\t\tTime: {:?}", point.time_unix_nano);
                            if point.start_time_unix_nano != 0 {
                                println!("\t\tStart Time: {:?}", point.start_time_unix_nano);
                            }
                        }
                    },
                    opentelemetry_proto::tonic::metrics::v1::metric::Data::Sum(sum) => {
                        println!("\tType: Sum");
                        println!("\tIs Monotonic: {}", sum.is_monotonic);
                        println!("\tAggregation Temporality: {:?}", sum.aggregation_temporality);
                        for point in &sum.data_points {
                            println!("\tDataPoint:");
                            println!("\t\tValue: {:?}", point.value);
                            if !point.attributes.is_empty() {
                                println!("\t\tAttributes:");
                                for attr in &point.attributes {
                                    if let Some(value) = &attr.value {
                                        println!("\t\t\t{} = {:?}", attr.key, value.value);
                                    }
                                }
                            }
                            println!("\t\tTime: {:?}", point.time_unix_nano);
                            if point.start_time_unix_nano != 0 {
                                println!("\t\tStart Time: {:?}", point.start_time_unix_nano);
                            }
                        }
                    },
                    opentelemetry_proto::tonic::metrics::v1::metric::Data::Histogram(hist) => {
                        println!("\tType: Histogram");
                        println!("\tAggregation Temporality: {:?}", hist.aggregation_temporality);
                        for point in &hist.data_points {
                            println!("\tDataPoint:");
                            println!("\t\tCount: {}", point.count);
                            println!("\t\tSum: {:?}", point.sum);
                            if !point.bucket_counts.is_empty() {
                                println!("\t\tBucket Counts: {:?}", point.bucket_counts);
                                println!("\t\tBucket Boundaries: {:?}", point.explicit_bounds);
                            }
                            if !point.attributes.is_empty() {
                                println!("\t\tAttributes:");
                                for attr in &point.attributes {
                                    if let Some(value) = &attr.value {
                                        println!("\t\t\t{} = {:?}", attr.key, value.value);
                                    }
                                }
                            }
                            println!("\t\tTime: {:?}", point.time_unix_nano);
                            if point.start_time_unix_nano != 0 {
                                println!("\t\tStart Time: {:?}", point.start_time_unix_nano);
                            }
                        }
                    },
                    _ => println!("\tOther metric type"),
                }
            },
            None => println!("\tNo data"),
        }
        println!("===============================");
    }
}

#[tonic::async_trait]
impl MetricsService for MetricsReceiver {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        let metrics = request.into_inner();
        let mut seen_metrics = self.seen_metrics.lock().unwrap();
        
        for resource_metrics in metrics.resource_metrics {
            for scope_metrics in &resource_metrics.scope_metrics {
                for metric in &scope_metrics.metrics {
                    if seen_metrics.insert(metric.name.clone()) {
                        println!("Discovered metric: {}", metric.name);
                    }
                    
                    if self.debug_mode {
                        self.print_debug_metric(&resource_metrics, scope_metrics, metric);
                    }
                }
            }
        }

        Ok(Response::new(ExportMetricsServiceResponse::default()))
    }
}

#[tokio::main]
async fn main() -> Result<(), DashboardError> {
    let args = Args::parse();

    let log_level = if args.debug { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(log_level)
        .init();

    let addr = args.address;
    let metrics_service = MetricsServiceServer::new(MetricsReceiver::new(args.debug));

    tracing::info!("Starting OTLP receiver on {}", addr);

    Server::builder()
        .add_service(metrics_service)
        .serve(addr)
        .await?;

    Ok(())
}
