mod collector;
mod topology;

use anyhow::Result;
use collector::{SysfsCollector, TelemetryCollector};
use std::path::Path;
use topology::SystemTopology;

fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

fn has_btf_support() -> bool {
    Path::new("/sys/kernel/btf/vmlinux").exists()
}

fn select_collector() -> Box<dyn TelemetryCollector> {
    if is_root() && has_btf_support() {
        //Placeholder for Phase 2
        println!("[SYSTEM] Elevated privileges detected(eBPF Engine implementation pending)");
        Box::new(SysfsCollector::new())
    } else {
        println!("[SYSTEM] No elevated privileges detected, falling back to sysfs collector");
        Box::new(SysfsCollector::new())
    }
}

fn main() -> Result<()> {
    // 1. Resolve Hardware
    println!("[INFO] Resolving System Topology...");
    let topo = SystemTopology::resolve()?;
    println!("[INFO] Detected {} Logical Cores", topo.cores.len());

    // 2. Initialize Telemetry
    let mut collector = select_collector();

    // 3. Start TUI (Placeholder for Ratatui Loop)
    println!("[INFO] Booting TUI...");

    Ok(())
}
