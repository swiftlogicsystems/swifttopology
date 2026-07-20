use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::mem::MaybeUninit;

// --- libbpf-rs Traits ---
use libbpf_rs::skel::{OpenSkel, Skel, SkelBuilder};
use libbpf_rs::MapCore;

mod swifttopology_bpf {
    include!(concat!(env!("OUT_DIR"), "/swifttopology.skel.rs"));
}
use swifttopology_bpf::*;

// --- Shared Data Structures ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadClassification {
    Idle,
    ComputeBound,
    MemoryBound,
    ContentionBound,
}

impl Default for WorkloadClassification {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, Default)]
pub struct CoreMetrics {
    pub cpu_id: u32,
    pub exec_pct: f64,
    pub wait_pct: Option<f64>,
    pub llc_miss_rate: Option<f64>,
    pub ipc: Option<f64>,
    pub current_pid: Option<u32>,
    pub classification: WorkloadClassification,
}

pub trait TelemetryCollector {
    fn update_metrics(&mut self) -> HashMap<u32, CoreMetrics>;
}

// --- Implementation 1: EbpfCollector (Phase 2) ---

pub struct EbpfCollector {
    // This now correctly holds a 'static skeleton
    skel: SwifttopologySkel<'static>,
    _perf_fds: Vec<i32>, // Keep the FDs open to keep the PMU counters active
}

impl EbpfCollector {
    pub fn init(num_cpus: u32) -> Result<Self> {
        let builder = SwifttopologySkelBuilder::default();

        // FIX: We allocate the OpenObject on the heap and "leak" it.
        // In a long-running CLI tool, this is perfectly fine as it happens once.
        // This provides the 'static lifetime the compiler is looking for.
        let open_obj = Box::leak(Box::new(MaybeUninit::uninit()));

        let open_skel = builder
            .open(open_obj)
            .context("Failed to open BPF skeleton")?;
        let mut skel = open_skel.load().context("Failed to load BPF skeleton")?;
        let mut perf_fds = Vec::new();

        // Bind the PMU counters to the perf event array
        for cpu in 0..num_cpus{
            // Index 0: Instructions
            // Index 1: Cycles
            // Index 2: L3 Misses
            let configs = [
                (libc::PERF_COUNT_HW_INSTRUCTIONS, &mut skel.maps.perf_instructions),
                (libc::PERF_COUNT_HW_CYCLES, &mut skel.maps.perf_cycles),
                (libc::PERF_COUNT_HW_L3_MISSES, &mut skel.maps.perf_l3_misses),
            ];
            for (config, map) in configs {
            let fd = open_perf_counter(cpu as i32, config as u64)?;
            map.update(&cpu.to_ne_bytes(), &(fd as i32).to_ne_bytes(), libbpf_rs::MapFlags::ANY)?;
            perf_fds.push(fd);
            }
        }
        skel.attach()?;
        Ok(Self { skel, _perf_fds: perf_fds })
    }


}

//Helper Function top open Hardware Counters via libc
fn open_perf_counter(cpu:i32, config: u64) -> Result<i32> {
    let mut attr = unsafe {std::mem::zeroed::<libc::perf_event_attr>()};
    attr.type_ = libc::PERF_TYPE_HARDWARE;
    attr.config = config;
    attr.size = std::mem::size_of::<libc::perf_event_attr>() as u32;
    attr.set_disabled(0);
    attr.set_pinned(1);

    let fd = unsafe { libc::perf_event_open(&mut attr, -1, cpu, -1, 0) };
    if fd < 0 { anyhow::bail!("perf_event_open failed"); }
    Ok(fd)
}

impl TelemetryCollector for EbpfCollector {
    fn update_metrics(&mut self) -> HashMap<u32, CoreMetrics> {
        let mut metrics = HashMap::new();
        let stats_map = &self.skel.maps.cpu_stats_map;

        // Iterate through cores (up to 256 or actual count)
        for cpu_id in 0..256u32 {
            let key = cpu_id.to_ne_bytes();

            if let Ok(Some(per_cpu_values)) =
                stats_map.lookup_percpu(&key, libbpf_rs::MapFlags::ANY)
            {
                if let Some(raw_stats) = per_cpu_values.get(cpu_id as usize) {
                    if raw_stats.len() >= 20 {
                        let pid_bytes: [u8; 4] = raw_stats[16..20].try_into().unwrap_or([0; 4]);
                        let pid = u32::from_ne_bytes(pid_bytes);

                        metrics.insert(
                            cpu_id,
                            CoreMetrics {
                                cpu_id,
                                current_pid: if pid > 0 { Some(pid) } else { None },
                                classification: WorkloadClassification::ComputeBound,
                                ..Default::default()
                            },
                        );
                    }
                }
            }
        }
        metrics
    }
}

// --- Implementation 2: SysfsCollector (Fallback) ---

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
        let content = fs::read_to_string("/proc/stat").unwrap_or_default();

        for line in content.lines() {
            if line.starts_with("cpu") && !line.contains("cpu ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 9 {
                    continue;
                }

                let cpu_id = parts[0][3..].parse().unwrap_or(0);
                let user: u64 = parts[1].parse().unwrap_or(0);
                let nice: u64 = parts[2].parse().unwrap_or(0);
                let system: u64 = parts[3].parse().unwrap_or(0);
                let idle: u64 = parts[4].parse().unwrap_or(0);
                let iowait: u64 = parts[5].parse().unwrap_or(0);
                let irq: u64 = parts[6].parse().unwrap_or(0);
                let softirq: u64 = parts[7].parse().unwrap_or(0);
                let steal: u64 = parts[8].parse().unwrap_or(0);

                let work = user + nice + system + irq + softirq + steal;
                let total = work + idle + iowait;

                if let Some(&(prev_work, prev_total)) = self.last_cpu_times.get(&cpu_id) {
                    let diff_work = work.saturating_sub(prev_work) as f64;
                    let diff_total = total.saturating_sub(prev_total) as f64;

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
                            classification: if usage < 2.0 {
                                WorkloadClassification::Idle
                            } else {
                                WorkloadClassification::ComputeBound
                            },
                            ..Default::default()
                        },
                    );
                }
                self.last_cpu_times.insert(cpu_id, (work, total));
            }
        }
        results
    }
}
