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
        " swift-topomap v0.1 | Host: {} | {} Cores | [h] help [q] quit ",
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
    // Help Overlay
    if app.show_help {
        draw_help_overlay(f);
    }
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
        Span::raw("Mem-Stall "), // Amber!
        Span::styled(" ■ ", Style::default().fg(COLOR_CONTENTION)),
        Span::raw("Contention"), // Crimson!
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
    let ipc = metric.and_then(|m| m.ipc).unwrap_or(0.0);
    let pid = metric.and_then(|m| m.current_pid);

    // Map ALL classifications to their brand colors
    let color = match metric.map(|m| m.classification).unwrap_or_default() {
        WorkloadClassification::Idle => COLOR_IDLE,
        WorkloadClassification::ComputeBound => COLOR_COMPUTE,
        WorkloadClassification::MemoryBound => COLOR_MEMORY, // Now Amber
        WorkloadClassification::ContentionBound => COLOR_CONTENTION, // Now Crimson
    };

    let (pid_label, ipc_label) = if usage > 0.5 {
        let p_str = match pid {
            Some(p) => format!("PID:{}", p),
            None => "IDLE".to_string(),
        };
        (p_str, format!("IPC:{:.2}", ipc))
    } else {
        ("IDLE".to_string(), "        ".to_string()) // Hide technicals when idle
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .padding(Padding::horizontal(1))
        .border_style(Style::default().fg(color))
        .title(format!(" CPU {} | {} | {} ", cpu_id, pid_label, ipc_label));

    let width = area.width.saturating_sub(15) as usize;
    let filled = ((usage / 100.0) * width as f64) as usize;
    let bar = format!(
        "{:>5.1}% [{}{}]",
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

fn draw_help_overlay(f: &mut Frame) {
    let area = centered_rect(80, 75, f.size());

    // SwiftLogic Orange: R:255, G:135, B:0
    let swiftlogic_orange = Color::Rgb(255, 135, 0);

    let block = Block::default()
        .title(" swift-topomap | Technical Reference ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .padding(Padding::uniform(1));

    f.render_widget(ratatui::widgets::Clear, area);

    let help_text = vec![
        Line::from(vec![Span::styled(
            "CORE CONTROLS",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::raw("  h / ?   "),
            Span::styled(
                "Toggle this reference screen",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::raw("  q / Esc "),
            Span::styled("Exit utility", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "OBSERVABILITY AT SPEED",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::raw("  Use "),
            Span::styled("-i <ms>", Style::default().fg(Color::Yellow)),
            Span::raw(" to adjust the sampling resolution."),
        ]),
        Line::from(vec![
            Span::styled("  • High-Perf: ", Style::default().fg(Color::Green)),
            Span::raw("-i 50 for ultra-fast microarchitectural monitoring."),
        ]),
        Line::from(vec![
            Span::styled("  • Remote:    ", Style::default().fg(Color::Blue)),
            Span::raw("-i 1000 for stable monitoring over slow SSH links."),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "THE 'EXPERT' SWITCH",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::raw("  Use "),
            Span::styled("--sysfs", Style::default().fg(Color::Yellow)),
            Span::raw(" to bypass the eBPF engine."),
        ]),
        Line::from(vec![Span::raw(
            "  Forces the standard Sysfs engine for debugging and kernel-compatibility",
        )]),
        Line::from(vec![Span::raw(
            "  verification, even when running with root privileges.",
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "MICROARCHITECTURAL CLASSIFICATION",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("  ■ Green: ", Style::default().fg(COLOR_COMPUTE)),
            Span::raw("Healthy Compute (High IPC)"),
        ]),
        Line::from(vec![
            Span::styled("  ■ Amber: ", Style::default().fg(COLOR_MEMORY)),
            Span::raw("Memory/Cache Stalled (Low IPC/Stalled)"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("Developed by "),
            Span::styled("SwiftLogic Systems", Style::default().fg(swiftlogic_orange)),
        ]),
        Line::from(vec![Span::styled(
            "www.swiftlogic.systems",
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    f.render_widget(
        Paragraph::new(help_text)
            .block(block)
            .alignment(Alignment::Left),
        area,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1])[1]
}
