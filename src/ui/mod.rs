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

        let pkg_cores: Vec<_> = app
            .topo
            .cores
            .iter()
            .filter(|c| c.package_id == pkg_id)
            .collect();
        let l3_caches: Vec<_> = app
            .topo
            .cache_blocks
            .iter()
            .filter(|c| c.level == 3)
            .collect();

        if l3_caches.is_empty() {
            draw_core_grid(f, inner_socket, &pkg_cores, app);
        } else {
            let l3_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    l3_caches
                        .iter()
                        .map(|_| Constraint::Percentage(100 / l3_caches.len() as u16))
                        .collect::<Vec<_>>(),
                )
                .split(inner_socket);

            for (j, cache) in l3_caches.iter().enumerate() {
                let shared_ids = parse_cpu_list(&cache.shared_cpus);
                let cores_in_cache: Vec<_> = pkg_cores
                    .iter()
                    .filter(|c| shared_ids.contains(&c.logical_id))
                    .cloned()
                    .collect();

                let cache_block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Double)
                    .title(format!(
                        " L3 Cache ({}) | Cores: {} ",
                        cache.size,
                        cores_in_cache.len()
                    ))
                    .border_style(Style::default().fg(Color::DarkGray));

                let inner_cache = cache_block.inner(l3_chunks[j]);
                f.render_widget(cache_block, l3_chunks[j]);

                draw_core_grid(f, inner_cache, &cores_in_cache, app);
            }
        }
    }
}

fn draw_core_grid(f: &mut Frame, area: Rect, cores: &[&crate::topology::CpuCore], app: &App) {
    if cores.is_empty() {
        return;
    }

    let cols = 2;
    let rows = (cores.len() as f32 / cols as f32).ceil() as u16;

    let row_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Percentage(100 / rows); rows as usize])
        .split(area);

    for (r, row_rect) in row_layout.iter().enumerate() {
        let col_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(*row_rect);

        for (c, col_rect) in col_layout.iter().enumerate() {
            let idx = r * cols as usize + c;
            if idx < cores.len() {
                draw_core(f, *col_rect, cores[idx].logical_id, app);
            }
        }
    }
}

fn draw_core(f: &mut Frame, area: Rect, cpu_id: u32, app: &App) {
    let metric = app.metrics.get(&cpu_id);
    let usage = metric.map(|m| m.exec_pct).unwrap_or(0.0);

    // Find L1 and L2 cache sizes for this specific CPU
    let l1_size = app
        .topo
        .cache_blocks
        .iter()
        .find(|c| c.level == 1 && parse_cpu_list(&c.shared_cpus).contains(&cpu_id))
        .map(|c| c.size.clone())
        .unwrap_or_else(|| "??".into());

    let l2_size = app
        .topo
        .cache_blocks
        .iter()
        .find(|c| c.level == 2 && parse_cpu_list(&c.shared_cpus).contains(&cpu_id))
        .map(|c| c.size.clone())
        .unwrap_or_else(|| "??".into());

    let color = match metric.map(|m| m.classification).unwrap_or_default() {
        WorkloadClassification::Idle => COLOR_IDLE,
        WorkloadClassification::ComputeBound => COLOR_COMPUTE,
        _ => COLOR_IDLE,
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .padding(Padding::horizontal(1))
        .border_style(Style::default().fg(color))
        // Title now shows the L1/L2 info for this core
        .title(format!(" CPU {} [L1:{} L2:{}] ", cpu_id, l1_size, l2_size));

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

fn parse_cpu_list(list: &str) -> Vec<u32> {
    let mut cpus = Vec::new();
    // Clean string of any spaces
    let clean_list = list.replace(' ', "");
    for part in clean_list.split(',') {
        if part.contains('-') {
            let range: Vec<&str> = part.split('-').collect();
            if range.len() == 2 {
                if let (Ok(start), Ok(end)) = (range[0].parse::<u32>(), range[1].parse::<u32>()) {
                    for i in start..=end {
                        cpus.push(i);
                    }
                }
            }
        } else if let Ok(id) = part.parse::<u32>() {
            cpus.push(id);
        }
    }
    cpus
}
