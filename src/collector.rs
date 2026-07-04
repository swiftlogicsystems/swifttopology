use std::collections::HashMap;

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
    last_stats: HashMap<u32, (u64, u64)>,
}
impl SysfsCollector {
    pub fn new() -> Self {
        Self {
            last_stats: HashMap::new(),
        }
    }
}

impl TelemetryCollector for SysfsCollector {
    fn update_metrics(&mut self) -> HashMap<u32, CoreMetrics> {
        let mut results = HashMap::new();
        // In a real implementation, parse /proc/stat here
        // For now, return empty or dummy metrics
        results
    }
}
