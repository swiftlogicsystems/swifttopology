use std::collections::hash_set::Difference;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadClassification {
    Idle,
    ComputeBound,
    MemoryBound,
    ContentionBound,
    Mixed,
}

#[derive(Debug, Clone, Default)]
pub struct CoreMetrics {
    pub cpu_id: u32,
    pub exec_pct: f64,
    pub wait_pct: Option<f64>,
    pub llc_miss_rate: Option<f64>,
    pub ipc: Option<f64>,
    pub classification: WorkloadClassification,
}

impl Default for WorkloadClassification {
    fn default() -> Self {
        Self::Idle
    }
}

pub trait TelemetryCollector {
    fn update_metrics(&mut self) -> HashMap<u32, CoreMetrics>;
}
///A Fallback collector that reads from /proc/stat
pub struct SysfsCollector {
    last_cpu_times: HashMap<u32, (u64, u64)>,
}
impl SysfsCollector {
    pub fn new() -> Self {
        Self {
            last_cpu_times: HashMap::new(),
        }
    }
}

impl TelemetryCollector for SysfsCollector {
    fn update_metrics(&mut self) -> HashMap<u32, CoreMetrics> {
        let mut results = HashMap::new();

        // Read /proc/stat and parse CPU times
        let content = fs::read_to_string("/proc/stat").unwrap_or_default();

        for line in content.lines() {
            // Get the cpu0, cpu1, cpu2, ... lines
            if line.starts_with("cpu") && !line.contains("cpu ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 5 {
                    continue;
                }
                let cpu_id = parts[0][3..].parse().unwrap_or(0);

                // Fields from /proc/stat: user, nice, system, idle, iowait, irq, softriq, steal
                let user: u64 = parts[1].parse().unwrap_or(0);
                let nice: u64 = parts[2].parse().unwrap_or(0);
                let system: u64 = parts[3].parse().unwrap_or(0);
                let idle: u64 = parts[4].parse().unwrap_or(0);
                let iowait: u64 = parts[5].parse().unwrap_or(0);
                let irq: u64 = parts[6].parse().unwrap_or(0);
                let softirq: u64 = parts[7].parse().unwrap_or(0);
                let steal: u64 = parts[8].parse().unwrap_or(0);

                let work = user + nice + system + steal + irq + softirq;
                let wait = idle + iowait + work;

                // Calculate Delta
                if let Some(&(pre_work, prev_total)) = self.last_cpu_times.get(&cpu_id) {
                    let diff_work = work.saturating_sub(pre_work) as f64;
                    let diff_total = wait.saturating_sub(prev_total) as f64;

                    let usage = if diff_total > 0.0 {
                        (diff_work / diff_total) * 100.0
                    } else {
                        0.0
                    };

                    results.insert(
                        cpu_id,
                        CoreMetrics {
                            cpu_id,
                            exec_pct: usage,
                            wait_pct: None,
                            llc_miss_rate: None,
                            ipc: None,
                            classification: if usage < 2.0 {
                                WorkloadClassification::Idle
                            } else {
                                WorkloadClassification::ComputeBound
                            },
                        },
                    );
                }

                self.last_cpu_times.insert(cpu_id, (work, wait));
            }
        }
        results
    }
}
