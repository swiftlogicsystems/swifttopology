use anyhow::{Context, Result};
use libbpf_rs::skel::{OpenSkel, Skel, SkelBuilder};
use libbpf_rs::MapCore;
use libc::clone_args;
use std::collections::HashMap;
use std::fs;
use std::mem::MaybeUninit;

// --- Linux Perf ABI Constants ---
const PERF_TYPE_HARDWARE: u32 = 0;
const PERF_COUNT_HW_CPU_CYCLES: u64 = 0;
const PERF_COUNT_HW_INSTRUCTIONS: u64 = 1;
const PERF_COUNT_HW_CACHE_MISSES: u64 = 5;

// --- Manual perf_event_attr definition for high portability ---
#[repr(C)]
#[derive(Copy, Clone)]
pub struct perf_event_attr {
    pub type_: u32,
    pub size: u32,
    pub config: u64,
    pub sample_period_or_freq: u64,
    pub sample_type: u64,
    pub read_format: u64,
    pub flags: u64,
    pub wakeup_events_or_watermark: u32,
    pub bp_type: u32,
    pub bp_addr: u64,
    pub bp_len: u64,
    pub branch_sample_type: u64,
    pub sample_regs_user: u64,
    pub sample_stack_user: u32,
    pub clockid: i32,
    pub sample_regs_intr: u64,
    pub aux_sample_size: u32,
    pub __reserved_2: u32,
}

// --- BPF Skeleton Integration ---
mod swifttopology_bpf {
    include!(concat!(env!("OUT_DIR"), "/swifttopology.skel.rs"));
}
use swifttopology_bpf::*;

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

pub struct EbpfCollector {
    skel: SwifttopologySkel<'static>,
    _perf_fds: Vec<i32>,
    // (Work, Total, BPF_Instructions, BPF_Cycles)
    last_raw: HashMap<u32, (u64, u64, u64, u64)>,
    update_metrics_history: HashMap<u32, CoreMetrics>,
}

impl EbpfCollector {
    pub fn init(num_cpus: u32) -> Result<Self> {
        let builder = SwifttopologySkelBuilder::default();
        let open_obj = Box::leak(Box::new(MaybeUninit::uninit()));
        let open_skel = builder
            .open(open_obj)
            .context("Failed to open BPF skeleton")?;
        let mut skel = open_skel.load().context("Failed to load BPF skeleton")?;

        let mut perf_fds = Vec::new();

        for cpu in 0..num_cpus {
            let configs = [
                (PERF_COUNT_HW_INSTRUCTIONS, &mut skel.maps.perf_instructions),
                (PERF_COUNT_HW_CPU_CYCLES, &mut skel.maps.perf_cycles),
                (PERF_COUNT_HW_CACHE_MISSES, &mut skel.maps.perf_l3_misses),
            ];

            for (config, map) in configs {
                let fd = open_perf_counter(cpu as i32, config)?;
                map.update(
                    &cpu.to_ne_bytes(),
                    &(fd as i32).to_ne_bytes(),
                    libbpf_rs::MapFlags::ANY,
                )?;
                perf_fds.push(fd);
            }
        }

        skel.attach().context("Failed to attach BPF probes")?;
        Ok(Self {
            skel,
            _perf_fds: perf_fds,
            last_raw: HashMap::new(),
            update_metrics_history: HashMap::new(),
        })
    }
}

impl TelemetryCollector for EbpfCollector {
    fn update_metrics(&mut self) -> HashMap<u32, CoreMetrics> {
        let mut metrics = HashMap::new();

        // 1. Get standard CPU usage (Stable data)
        let proc_stat = fs::read_to_string("/proc/stat").unwrap_or_default();
        let mut usage_map = HashMap::new();
        for line in proc_stat.lines() {
            if line.starts_with("cpu") && !line.contains("cpu ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 9 {
                    continue;
                }
                let cpu_id = parts[0][3..].parse().unwrap_or(0);
                let work: u64 = parts[1..4]
                    .iter()
                    .chain(parts[6..9].iter())
                    .filter_map(|s| s.parse::<u64>().ok())
                    .sum();
                let idle: u64 = parts[4].parse().unwrap_or(0) + parts[5].parse().unwrap_or(0);
                let total = work + idle;

                if let Some((prev_work, prev_total, _, _)) = self.last_raw.get(&cpu_id).cloned() {
                    let d_work = work.saturating_sub(prev_work) as f64;
                    let d_total = (total.saturating_sub(prev_total)) as f64;
                    let usage = if d_total > 0.0 {
                        (d_work / d_total) * 100.0
                    } else {
                        0.0
                    };
                    usage_map.insert(cpu_id, (usage, work, total));
                } else {
                    usage_map.insert(cpu_id, (0.0, work, total));
                }
            }
        }

        // 2. Get Deep Metrics with Persistent State
        let stats_map = &self.skel.maps.cpu_stats_map;
        for cpu_id in 0..256u32 {
            let key = cpu_id.to_ne_bytes();
            if let Ok(Some(per_cpu_values)) =
                stats_map.lookup_percpu(&key, libbpf_rs::MapFlags::ANY)
            {
                if let Some(raw) = per_cpu_values.get(cpu_id as usize) {
                    if raw.len() >= 32 {
                        let inst = u64::from_ne_bytes(raw[0..8].try_into().unwrap());
                        let cycl = u64::from_ne_bytes(raw[8..16].try_into().unwrap());
                        let miss = u64::from_ne_bytes(raw[16..24].try_into().unwrap());
                        let pid = u32::from_ne_bytes(raw[24..28].try_into().unwrap());

                        let (usage, cur_work, cur_total) =
                            usage_map.get(&cpu_id).cloned().unwrap_or((0.0, 0, 0));

                        // Default values
                        let mut ipc = 0.0;
                        let mut classification = WorkloadClassification::Idle;

                        if let Some((_, _, p_inst, p_cycl)) = self.last_raw.get(&cpu_id).cloned() {
                            let d_inst = inst.saturating_sub(p_inst) as f64;
                            let d_cycl = cycl.saturating_sub(p_cycl) as f64;

                            if d_cycl > 1000.0 {
                                // WE HAVE NEW DATA: Calculate new IPC
                                ipc = d_inst / d_cycl;

                                if usage < 2.0 {
                                    classification = WorkloadClassification::Idle;
                                } else if ipc < 0.5 && usage > 50.0 {
                                    classification = WorkloadClassification::MemoryBound;
                                } else {
                                    classification = WorkloadClassification::ComputeBound;
                                }
                            } else {
                                if let Some(prev_metric) = self.update_metrics_history.get(&cpu_id)
                                {
                                    ipc = prev_metric.ipc.unwrap_or(0.0);
                                    classification = prev_metric.classification;
                                }
                            }
                        }

                        self.last_raw
                            .insert(cpu_id, (cur_work, cur_total, inst, cycl));

                        let core_metric = CoreMetrics {
                            cpu_id,
                            exec_pct: usage,
                            ipc: Some(ipc),
                            llc_miss_rate: Some(miss as f64),
                            current_pid: if pid > 0 { Some(pid) } else { None },
                            classification,
                            ..Default::default()
                        };

                        // Store this for next tick's fallback
                        metrics.insert(cpu_id, core_metric.clone());
                    }
                }
            }
        }
        // Save the metrics globally in the collector so we can refer back to them
        self.update_metrics_history = metrics.clone();
        metrics
    }
}

fn open_perf_counter(cpu: i32, config: u64) -> Result<i32> {
    // Using our locally defined perf_event_attr
    let mut attr = unsafe { std::mem::zeroed::<perf_event_attr>() };
    attr.type_ = PERF_TYPE_HARDWARE;
    attr.size = std::mem::size_of::<perf_event_attr>() as u32;
    attr.config = config;

    // Set 'disabled' to 0 and 'pinned' to 1 using bits.
    // In the Linux struct, these are part of a bitfield in the 'flags' or equivalent area.
    // For hardware counters, the kernel defaults work well if we just set size/type/config.

    let fd = unsafe { libc::syscall(libc::SYS_perf_event_open, &attr, -1, cpu, -1, 0) } as i32;

    if fd < 0 {
        anyhow::bail!(
            "perf_event_open failed: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(fd)
}

// --- Implementation 2: SysfsCollector ---
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
