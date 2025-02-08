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
   widgets::{Block, Borders, List, ListItem, ListState},
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
   ui_tx: mpsc::UnboundedSender<UiMessage>,
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
   discovered_metrics: Vec<String>,
   recent_updates: VecDeque<String>,
   list_state: ListState,
   selected_metric: Option<String>,
}

impl TuiState {
   fn new() -> Self {
       Self {
           discovered_metrics: Vec::new(),
           recent_updates: VecDeque::with_capacity(100),
           list_state: ListState::default(),
           selected_metric: None,
       }
   }

   fn add_metric(&mut self, metric: String) {
       if !self.discovered_metrics.contains(&metric) {
           self.discovered_metrics.push(metric);
           self.discovered_metrics.sort();
           if self.list_state.selected().is_none() {
               self.list_state.select(Some(0));
           }
       }
   }

   fn add_update(&mut self, update: String) {
       if let Some(selected) = &self.selected_metric {
           if update.starts_with(selected) {
               self.recent_updates.push_front(update);
               if self.recent_updates.len() > 100 {
                   self.recent_updates.pop_back();
               }
           }
       } else {
           self.recent_updates.push_front(update);
           if self.recent_updates.len() > 100 {
               self.recent_updates.pop_back();
           }
       }
   }

   fn next(&mut self) {
       let i = match self.list_state.selected() {
           Some(i) => {
               if i >= self.discovered_metrics.len() - 1 {
                   0
               } else {
                   i + 1
               }
           }
           None => 0,
       };
       self.list_state.select(Some(i));
   }

   fn previous(&mut self) {
       let i = match self.list_state.selected() {
           Some(i) => {
               if i == 0 {
                   self.discovered_metrics.len() - 1
               } else {
                   i - 1
               }
           }
           None => 0,
       };
       self.list_state.select(Some(i));
   }

   fn toggle_selected_metric(&mut self) {
       if let Some(index) = self.list_state.selected() {
           if let Some(metric) = self.discovered_metrics.get(index) {
               if self.selected_metric.as_ref().map_or(false, |m| m == metric) {
                   self.selected_metric = None;
                   self.recent_updates.clear();
               } else {
                   self.selected_metric = Some(metric.clone());
                   self.recent_updates.clear();
               }
           }
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
               .map(|m| {
                   let style = if Some(m) == state.selected_metric.as_ref() {
                       Style::default().fg(Color::Yellow)
                   } else {
                       Style::default()
                   };
                   ListItem::new(m.as_str()).style(style)
               })
               .collect();

           let title = if state.selected_metric.is_some() {
               "Discovered Metrics [j/k to navigate, Enter to unfilter]"
           } else {
               "Discovered Metrics [j/k to navigate, Enter to filter]"
           };

           let metrics_list = List::new(metrics)
               .block(Block::default().title(title).borders(Borders::ALL))
               .highlight_style(Style::default().bg(Color::White).fg(Color::Black));
           f.render_stateful_widget(metrics_list, chunks[0], &mut state.list_state);

           let updates_title = if let Some(metric) = &state.selected_metric {
               format!("Recent Updates (Filtered: {})", metric)
           } else {
               "Recent Updates (All Metrics)".to_string()
           };

           let updates: Vec<ListItem> = state.recent_updates.iter()
               .map(|u| ListItem::new(u.as_str()))
               .collect();
           let updates_list = List::new(updates)
               .block(Block::default().title(updates_title).borders(Borders::ALL));
           f.render_widget(updates_list, chunks[1]);
       })?;

       if event::poll(std::time::Duration::from_millis(100))? {
           if let Event::Key(key) = event::read()? {
               match key.code {
                   KeyCode::Char('q') => break,
                   KeyCode::Char('j') => state.next(),
                   KeyCode::Char('k') => state.previous(),
                   KeyCode::Enter => state.toggle_selected_metric(),
                   _ => {}
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

   let (tx, rx) = mpsc::unbounded_channel();
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
