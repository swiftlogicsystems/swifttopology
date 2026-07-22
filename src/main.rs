mod collector;
mod topology;
mod ui;

use crate::collector::{EbpfCollector, SysfsCollector, TelemetryCollector};
use crate::ui::app::App;
use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};

fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

fn has_btf_support() -> bool {
    Path::new("/sys/kernel/btf/vmlinux").exists()
}

fn select_collector(num_cpus: u32) -> Box<dyn TelemetryCollector> {
    if is_root() && has_btf_support() {
        match EbpfCollector::init(num_cpus) {
            Ok(collector) => Box::new(collector),
            Err(_) => Box::new(SysfsCollector::new()),
        }
    } else {
        Box::new(SysfsCollector::new())
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Refresh interval in milliseconds
    #[arg(short, long, default_value_t = 200)]
    interval: u64,

    /// Force Sysfs collector (ignore eBPF)
    #[arg(short, long, default_value_t = false)]
    sysfs: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    // Initial System Discovery
    let topo = topology::SystemTopology::resolve()?;
    let num_cpus = topo.cores.len() as u32;

    // Logic: Use sysfs if forced, otherwise negotiate
    let mut collector: Box<dyn TelemetryCollector> = if args.sysfs {
        Box::new(SysfsCollector::new())
    } else {
        select_collector(num_cpus)
    };

    // 2. Initialize App state
    let mut app = App::new(topo);

    // 3. Start TUI (Placeholder for Ratatui Loop)
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // main Event loop
    let tick_rate = Duration::from_millis(200);
    let mut last_tick = Instant::now();

    while !app.should_quit {
        // Update data via collector
        app.update_metrics(collector.update_metrics());

        // Draw UI
        terminal.draw(|f| ui::render(f, &app))?;

        //Handle Input
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => app.quit(),
                    KeyCode::Char('h') | KeyCode::Char('?') => app.toggle_help(),
                    _ => {}
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }
    // 5. Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
