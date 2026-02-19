use crate::error::DashboardError;
use crate::metrics::{MetricPoint, UiMessage};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Axis, Block, Borders, Chart, Dataset, List, ListItem, ListState},
    Terminal,
};
use std::collections::{HashMap, VecDeque};
use std::io;
use tokio::sync::mpsc::UnboundedReceiver;
use chrono::{NaiveDateTime, Timelike};

const MAX_POINTS: usize = 100;

pub struct TuiState {
    discovered_metrics: Vec<String>,
    recent_updates: VecDeque<String>,
    list_state: ListState,
    selected_metric: Option<String>,
    metric_data: HashMap<String, VecDeque<MetricPoint>>,
    show_graph: bool,
}

impl TuiState {
    fn new() -> Self {
        Self {
            discovered_metrics: Vec::new(),
            recent_updates: VecDeque::with_capacity(100),
            list_state: ListState::default(),
            selected_metric: None,
            metric_data: HashMap::new(),
            show_graph: false,
        }
    }

    fn add_metric(&mut self, metric: String) {
        if !self.discovered_metrics.contains(&metric) {
            self.discovered_metrics.push(metric.clone());
            self.discovered_metrics.sort();
            self.metric_data
                .insert(metric, VecDeque::with_capacity(MAX_POINTS));
            if self.list_state.selected().is_none() {
                self.list_state.select(Some(0));
            }
        }
    }

    fn add_metric_point(&mut self, name: String, point: MetricPoint) {
        if let Some(points) = self.metric_data.get_mut(&name) {
            points.push_back(point);
            if points.len() > MAX_POINTS {
                points.pop_front();
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
                    self.show_graph = false;
                    self.recent_updates.clear();
                } else {
                    self.selected_metric = Some(metric.clone());
                    self.show_graph = true;
                    self.recent_updates.clear();
                }
            }
        }
    }

    fn render_graph(&self, metric_name: &String, area: Rect, frame: &mut Frame) {
        if let Some(points) = self.metric_data.get(metric_name) {
            let data: Vec<(f64, f64)> = points
                .iter()
                .map(|point| (point.timestamp as f64, point.value))
                .collect();

            if !data.is_empty() {
                let min_x = data.first().map(|p| p.0).unwrap_or(0.0);
                let max_x = data.last().map(|p| p.0).unwrap_or(0.0);
                let min_y = data.iter().map(|p| p.1).reduce(f64::min).unwrap_or(0.0);
                let max_y = data.iter().map(|p| p.1).reduce(f64::max).unwrap_or(0.0);

                // Create labels for Y axis
                let y_labels = vec![
                    format!("{:.2}", min_y),
                    format!("{:.2}", (min_y + max_y) / 2.0),
                    format!("{:.2}", max_y),
                ]
                .into_iter()
                .map(|s| Span::raw(s))
                .collect::<Vec<Span>>();

                // Create labels for X axis with formatted timestamps
                let x_labels = vec![min_x, (min_x + max_x) / 2.0, max_x]
                    .into_iter()
                    .map(|ts| {
                        let datetime = NaiveDateTime::from_timestamp(ts as i64, 0);
                        let formatted_time = format!("{:02}:{:02}:{:02}", datetime.hour(), datetime.minute(), datetime.second());
                        Span::raw(formatted_time)
                    })
                    .collect::<Vec<Span>>();

                let dataset = Dataset::default()
                    .name(metric_name.clone())
                    .marker(symbols::Marker::Braille)
                    .graph_type(ratatui::widgets::GraphType::Line)
                    .data(&data);

                let chart = Chart::new(vec![dataset])
                    .block(
                        Block::default()
                            .title(format!("Metric: {}", metric_name))
                            .borders(Borders::ALL),
                    )
                    .x_axis(
                        Axis::default()
                            .title("Time (hh:mm:ss)")
                            .bounds([min_x, max_x])
                            .labels(x_labels),
                    )
                    .y_axis(
                        Axis::default()
                            .title("Value")
                            .bounds([min_y, max_y])
                            .labels(y_labels),
                    );

                frame.render_widget(chart, area);
            }
        }
    }
}
pub async fn run_tui(mut rx: UnboundedReceiver<UiMessage>) -> Result<(), DashboardError> {
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
                UiMessage::MetricDataPoint { name, point } => state.add_metric_point(name, point),
            }
        }

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
                .split(f.size());

            let metrics: Vec<ListItem> = state
                .discovered_metrics
                .iter()
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

            if state.show_graph {
                if let Some(metric_name) = &state.selected_metric {
                    state.render_graph(metric_name, chunks[1], f);
                }
            } else {
                let updates_title = if let Some(metric) = &state.selected_metric {
                    format!("Recent Updates (Filtered: {})", metric)
                } else {
                    "Recent Updates (All Metrics)".to_string()
                };

                let updates: Vec<ListItem> = state
                    .recent_updates
                    .iter()
                    .map(|u| ListItem::new(u.as_str()))
                    .collect();
                let updates_list = List::new(updates)
                    .block(Block::default().title(updates_title).borders(Borders::ALL));
                f.render_widget(updates_list, chunks[1]);
            }
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
