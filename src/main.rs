mod collector;
mod topology;
mod ui;

use crate::collector::{SysfsCollector, TelemetryCollector};
use crate::ui::app::App;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use libc::tolower;
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

fn select_collector() -> Box<dyn TelemetryCollector> {
    if is_root() && has_btf_support() {
        Box::new(SysfsCollector::new())
    } else {
        Box::new(SysfsCollector::new())
    }
}

fn main() -> Result<()> {
    // Initial System Discovery
    let topo = topology::SystemTopology::resolve()?;
    // DEBUG PRINT
    println!("[INFO] Detected {} Logical Cores", topo.cores.len());
    println!("[INFO] Detected {} Cache Blocks", topo.cache_blocks.len());
    for cb in &topo.cache_blocks {
        println!(
            "  -> L{} Cache: Size={}, CPUs={}",
            cb.level, cb.size, cb.shared_cpus
        );
    }

    // Give you 2 seconds to read the output before TUI starts
    std::thread::sleep(std::time::Duration::from_secs(2));
    let mut collector = select_collector();

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
