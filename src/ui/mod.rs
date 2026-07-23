pub mod app;

use crate::collector::WorkloadClassification;
use crate::ui::app::App;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph},
};

// SwiftLogic Brand Colors
const COLOR_IDLE: Color = Color::Rgb(49, 50, 68);
const COLOR_COMPUTE: Color = Color::Rgb(166, 227, 161);
const COLOR_MEMORY: Color = Color::Rgb(249, 226, 175);
const COLOR_CONTENTION: Color = Color::Rgb(243, 139, 168);
const SWIFTLOGIC_ORANGE: Color = Color::Rgb(255, 135, 0);

pub fn render(f: &mut Frame, app: &App) {
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(f.size());

    draw_header(f, main_layout[0], app);
    draw_topology_map(f, main_layout[1], app);
    draw_bottom_bar(f, main_layout[2]);

    if app.show_help {
        draw_help_popup(f);
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
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
    f.render_widget(header, area);
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
        let mut inner_socket = socket_block.inner(socket_chunks[i]);
        f.render_widget(socket_block, socket_chunks[i]);

        // 1. Draw NUMA RAM Bar
        if let Some(numa) = app.topo.numa_nodes.iter().find(|n| n.id == pkg_id) {
            let used_kb = numa.total_kb.saturating_sub(numa.free_kb);
            let usage_ratio = if numa.total_kb > 0 {
                used_kb as f64 / numa.total_kb as f64
            } else {
                0.0
            };
            let mem_text = format!(
                " RAM: {:.1} / {:.1} GB ({:.0}%) ",
                used_kb as f64 / 1024.0 / 1024.0,
                numa.total_kb as f64 / 1024.0 / 1024.0,
                usage_ratio * 100.0
            );

            let bar_width = 10;
            let filled = (usage_ratio * bar_width as f64) as usize;
            let gauge = format!("[{}{}]", "█".repeat(filled), "░".repeat(bar_width - filled));

            let mem_line = Line::from(vec![
                Span::raw(mem_text),
                Span::styled(
                    gauge,
                    Style::default().fg(if usage_ratio > 0.8 {
                        Color::Red
                    } else {
                        Color::Cyan
                    }),
                ),
            ]);

            f.render_widget(
                Paragraph::new(mem_line),
                Rect {
                    x: inner_socket.x + 2,
                    y: inner_socket.y,
                    width: inner_socket.width - 4,
                    height: 1,
                },
            );
            inner_socket.y += 1;
            inner_socket.height = inner_socket.height.saturating_sub(1);
        }

        // 2. Draw Cores (with optional L3 Nesting)
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
                    .title(format!(" L3 Cache ({}) ", cache.size))
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
        WorkloadClassification::MemoryBound => COLOR_MEMORY,
        WorkloadClassification::ContentionBound => COLOR_CONTENTION,
    };

    let (pid_label, ipc_label) = if usage > 0.5 {
        (
            match pid {
                Some(p) => format!("PID:{}", p),
                None => "IDLE".into(),
            },
            format!("IPC:{:.2}", ipc),
        )
    } else {
        ("IDLE".into(), "        ".into())
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .padding(Padding::horizontal(1))
        .border_style(Style::default().fg(color))
        .title(format!(" CPU {} | {} | {} ", cpu_id, pid_label, ipc_label));

    let width = area.width.saturating_sub(15) as usize;
    let filled = ((usage / 100.0) * width as f64) as usize;
    let bar = format!(
        "{:>5.1}% [{}{}] L1:{} L2:{}",
        usage,
        "█".repeat(filled),
        " ".repeat(width.saturating_sub(filled)),
        l1_size,
        l2_size
    );

    f.render_widget(
        Paragraph::new(bar)
            .block(block)
            .style(Style::default().fg(color)),
        area,
    );
}

fn draw_help_popup(f: &mut Frame) {
    let area = centered_rect(80, 75, f.size());
    let block = Block::default()
        .title(" swift-topomap | Technical Reference ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .padding(Padding::uniform(1));
    f.render_widget(Clear, area);

    let help_text = vec![
        Line::from(vec![Span::styled(
            "CORE CONTROLS",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  h / ?   - Toggle this reference screen"),
        Line::from("  q / Esc - Exit utility"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "OBSERVABILITY AT SPEED",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  Use -i <ms> to adjust sampling resolution (e.g., -i 50 for high-perf)."),
        Line::from(""),
        Line::from(vec![Span::styled(
            "THE 'EXPERT' SWITCH",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  Use --sysfs to bypass eBPF and force the standard kernel engine."),
        Line::from(""),
        Line::from(vec![
            Span::styled("Developed by ", Style::default().fg(SWIFTLOGIC_ORANGE)),
            Span::styled("SwiftLogic Systems", Style::default().fg(SWIFTLOGIC_ORANGE)),
        ]),
    ];
    f.render_widget(Paragraph::new(help_text).block(block), area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn parse_cpu_list(list: &str) -> Vec<u32> {
    let mut cpus = Vec::new();
    for part in list.replace(' ', "").split(',') {
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
