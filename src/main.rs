use clap::Parser;
use std::{collections::{HashSet, VecDeque}, net::SocketAddr};
use thiserror::Error;
use tonic::{transport::Server, Request, Response, Status};
use opentelemetry_proto::tonic::collector::metrics::v1::{
    metrics_service_server::{MetricsService, MetricsServiceServer},
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem},
    Terminal,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io;
use tokio::sync::{mpsc, Mutex as TokioMutex};

#[derive(Error, Debug)]
pub enum DashboardError {
    #[error("Failed to start server: {0}")]
    ServerError(#[from] tonic::transport::Error),
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
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
enum UiMessage {
    NewMetric(String),
    MetricUpdate(String),
}

struct MetricsReceiver {
    seen_metrics: TokioMutex<HashSet<String>>,
    debug_mode: bool,
    ui_tx: mpsc::UnboundedSender<UiMessage>, // Changed to UnboundedSender
}

impl MetricsReceiver {
    fn new(debug_mode: bool, ui_tx: mpsc::UnboundedSender<UiMessage>) -> Self {
        Self {
            seen_metrics: TokioMutex::new(HashSet::new()),
            debug_mode,
            ui_tx,
        }
    }

    async fn send_metric_update(&self, metric_name: &str, details: String) {
        if let Err(e) = self.ui_tx.send(UiMessage::MetricUpdate(
            format!("{}: {}", metric_name, details)
        )) {
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
                        Some(data) => {
                            match data {
                                opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(gauge) => {
                                    for point in &gauge.data_points {
                                        self.send_metric_update(&metric.name, 
                                            format!("= {:?}", point.value)
                                        ).await;
                                    }
                                },
                                opentelemetry_proto::tonic::metrics::v1::metric::Data::Sum(sum) => {
                                    for point in &sum.data_points {
                                        self.send_metric_update(&metric.name, 
                                            format!("= {:?}", point.value)
                                        ).await;
                                    }
                                },
                                opentelemetry_proto::tonic::metrics::v1::metric::Data::Histogram(hist) => {
                                    for point in &hist.data_points {
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

struct TuiState {
    discovered_metrics: HashSet<String>,
    recent_updates: VecDeque<String>,
}

impl TuiState {
    fn new() -> Self {
        Self {
            discovered_metrics: HashSet::new(),
            recent_updates: VecDeque::with_capacity(100),
        }
    }

    fn add_metric(&mut self, metric: String) {
        self.discovered_metrics.insert(metric);
    }

    fn add_update(&mut self, update: String) {
        self.recent_updates.push_front(update);
        if self.recent_updates.len() > 100 {
            self.recent_updates.pop_back();
        }
    }
}

async fn run_tui(mut rx: mpsc::UnboundedReceiver<UiMessage>) -> Result<(), DashboardError> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = TuiState::new();

    loop {
        // Process all pending messages
        while let Ok(message) = rx.try_recv() {
            match message {
                UiMessage::NewMetric(metric) => state.add_metric(metric),
                UiMessage::MetricUpdate(update) => state.add_update(update),
            }
        }

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(30),
                    Constraint::Percentage(70),
                ].as_ref())
                .split(f.size());

            let metrics: Vec<ListItem> = state.discovered_metrics.iter()
                .map(|m| ListItem::new(m.as_str()))
                .collect();
            let metrics_list = List::new(metrics)
                .block(Block::default().title("Discovered Metrics").borders(Borders::ALL));
            f.render_widget(metrics_list, chunks[0]);

            let updates: Vec<ListItem> = state.recent_updates.iter()
                .map(|u| ListItem::new(u.as_str()))
                .collect();
            let updates_list = List::new(updates)
                .block(Block::default().title("Recent Updates").borders(Borders::ALL));
            f.render_widget(updates_list, chunks[1]);
        })?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), DashboardError> {
    let args = Args::parse();

    let log_level = if args.debug { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(log_level)
        .init();

    let (tx, rx) = mpsc::unbounded_channel(); // Changed to unbounded channel
    let tui_handle = tokio::spawn(run_tui(rx));

    let addr = args.address;
    let metrics_service = MetricsServiceServer::new(MetricsReceiver::new(args.debug, tx));

    tracing::info!("Starting OTLP receiver on {}", addr);

    let server_handle = tokio::spawn(
        Server::builder()
            .add_service(metrics_service)
            .serve(addr)
    );

    tokio::select! {
        _ = tui_handle => println!("TUI closed"),
        _ = server_handle => println!("Server closed"),
    }

    Ok(())
}
