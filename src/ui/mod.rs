pub mod app;

use crate::collector::WorkloadClassification;
use crate::ui::app::App;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Padding, Paragraph},
};

const COLOR_IDLE: Color = Color::Rgb(49, 50, 68);
const COLOR_COMPUTE: Color = Color::Rgb(166, 227, 161);
const COLOR_MEMORY: Color = Color::Rgb(249, 226, 175);
const COLOR_CONTENTION: Color = Color::Rgb(243, 139, 168);

pub fn render(f: &mut Frame, app: &App) {
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(f.size());

    // Header: Dynamic Hostname
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "localhost".into());
    let header_text = format!(
        " swift-topomap v0.1 | Host: {} | {} Cores | 'q' to quit ",
        hostname,
        app.topo.cores.len()
    );
    let header = Paragraph::new(header_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .alignment(Alignment::Center);
    f.render_widget(header, main_layout[0]);

    draw_topology_map(f, main_layout[1], app);
    draw_bottom_bar(f, main_layout[2]);
}

fn draw_bottom_bar(f: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(25)])
        .split(area);

    let legend = Line::from(vec![
        Span::styled(" ■ ", Style::default().fg(COLOR_IDLE)),
        Span::raw("Idle "),
        Span::styled(" ■ ", Style::default().fg(COLOR_COMPUTE)),
        Span::raw("Compute "),
        Span::styled(" ■ ", Style::default().fg(COLOR_MEMORY)),
        Span::raw("Mem-Stall "),
        Span::styled(" ■ ", Style::default().fg(COLOR_CONTENTION)),
        Span::raw("Contention"),
    ]);

    f.render_widget(
        Paragraph::new(legend).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Metrics Legend "),
        ),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new("swiftlogic.systems")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .alignment(Alignment::Center),
        chunks[1],
    );
}

fn draw_topology_map(f: &mut Frame, area: Rect, app: &App) {
    // 1. Group by Sockets
    let mut package_ids: Vec<u32> = app.topo.cores.iter().map(|c| c.package_id).collect();
    package_ids.sort();
    package_ids.dedup();

    let socket_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            package_ids
                .iter()
                .map(|_| Constraint::Percentage(100 / package_ids.len() as u16))
                .collect::<Vec<_>>(),
        )
        .split(area);

    for (i, &pkg_id) in package_ids.iter().enumerate() {
        let socket_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Socket #{} ", pkg_id));
        let inner_socket = socket_block.inner(socket_chunks[i]);
        f.render_widget(socket_block, socket_chunks[i]);

        // 2. Nesting: Find L3 Caches for this Socket
        let l3_caches: Vec<_> = app
            .topo
            .cache_blocks
            .iter()
            .filter(|c| c.level == 3)
            .collect();

        let l3_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                l3_caches
                    .iter()
                    .map(|_| Constraint::Min(5))
                    .collect::<Vec<_>>(),
            )
            .split(inner_socket);

        for (j, cache) in l3_caches.iter().enumerate() {
            let cache_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .title(format!(" L3 Cache ({}) ", cache.size))
                .border_style(Style::default().fg(Color::DarkGray));

            let inner_cache = cache_block.inner(l3_chunks[j]);
            f.render_widget(cache_block, l3_chunks[j]);

            // 3. Draw Cores that share THIS L3 Cache
            // (We parse the shared_cpus string, e.g., "0-3")
            let shared_ids = parse_cpu_list(&cache.shared_cpus);
            let cores_in_cache: Vec<_> = app
                .topo
                .cores
                .iter()
                .filter(|c| shared_ids.contains(&c.logical_id))
                .collect();

            let core_rows = (cores_in_cache.len() as f32 / 2.0).ceil() as u16;
            let core_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![Constraint::Length(3); core_rows as usize])
                .split(inner_cache);

            for (r, row_rect) in core_layout.iter().enumerate() {
                let col_layout = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(*row_rect);

                for (c, col_rect) in col_layout.iter().enumerate() {
                    let idx = r * 2 + c;
                    if idx < cores_in_cache.len() {
                        draw_core(f, *col_rect, cores_in_cache[idx].logical_id, app);
                    }
                }
            }
        }
    }
}

fn draw_core(f: &mut Frame, area: Rect, cpu_id: u32, app: &App) {
    let metric = app.metrics.get(&cpu_id);
    let usage = metric.map(|m| m.exec_pct).unwrap_or(0.0);

    let color = match metric.map(|m| m.classification).unwrap_or_default() {
        WorkloadClassification::Idle => COLOR_IDLE,
        WorkloadClassification::ComputeBound => COLOR_COMPUTE,
        WorkloadClassification::MemoryBound => COLOR_MEMORY,
        WorkloadClassification::ContentionBound => COLOR_CONTENTION,
        _ => COLOR_IDLE,
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .padding(Padding::horizontal(1))
        .border_style(Style::default().fg(color))
        .title(format!(" CPU {} ", cpu_id));

    // Gauge calculation
    let width = area.width.saturating_sub(12) as usize;
    let filled = ((usage / 100.0) * width as f64) as usize;
    let bar = format!(
        "{:.1}% [{}{}]",
        usage,
        "█".repeat(filled),
        " ".repeat(width.saturating_sub(filled))
    );

    f.render_widget(
        Paragraph::new(bar)
            .block(block)
            .style(Style::default().fg(color)),
        area,
    );
}

// Simple helper to parse strings like "0-3" or "0,2,4"
fn parse_cpu_list(list: &str) -> Vec<u32> {
    let mut cpus = Vec::new();
    for part in list.split(',') {
        if part.contains('-') {
            let range: Vec<&str> = part.split('-').collect();
            if let (Ok(start), Ok(end)) = (range[0].parse::<u32>(), range[1].parse::<u32>()) {
                for i in start..=end {
                    cpus.push(i);
                }
            }
        } else if let Ok(id) = part.parse::<u32>() {
            cpus.push(id);
        }
    }
    cpus
}
