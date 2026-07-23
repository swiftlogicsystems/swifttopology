use anyhow::Result;
use std::collections::HashSet;
use std::fs;

#[derive(Debug, Clone)]
pub struct CpuCore {
    pub logical_id: u32,
    pub physical_id: u32,
    pub package_id: u32,
}

#[derive(Debug, Clone)]
pub struct CacheBlock {
    pub level: u32,
    pub size: String,
    pub type_name: String,
    pub shared_cpus: String,
}

#[derive(Debug, Clone)]
pub struct NumaNode {
    pub id: u32,
    pub total_kb: u64,
    pub free_kb: u64, // This represents "Actual Available" (Free + Inactive + Reclaimable)
}

#[derive(Debug, Clone)]
pub struct SystemTopology {
    pub cores: Vec<CpuCore>,
    pub cache_blocks: Vec<CacheBlock>,
    pub numa_nodes: Vec<NumaNode>,
}

impl SystemTopology {
    pub fn resolve() -> Result<Self> {
        let mut cores = Vec::new();
        let mut cache_blocks = Vec::new();
        let mut numa_nodes = Vec::new();
        let mut seen_caches = HashSet::new();

        let cpu_root = "/sys/devices/system/cpu";
        let entries = fs::read_dir(cpu_root)?;

        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().into_string().unwrap_or_default();

            if name.starts_with("cpu") && name[3..].chars().all(|c| c.is_numeric()) {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let id: u32 = name[3..].parse().unwrap_or(0);

                // 1. Resolve Core Topology
                let topo_path = path.join("topology");
                if topo_path.is_dir() {
                    let package_id = fs::read_to_string(topo_path.join("physical_package_id"))
                        .map(|s| s.trim().parse().unwrap_or(0))
                        .unwrap_or(0);
                    let core_id = fs::read_to_string(topo_path.join("core_id"))
                        .map(|s| s.trim().parse().unwrap_or(0))
                        .unwrap_or(0);

                    cores.push(CpuCore {
                        logical_id: id,
                        physical_id: core_id,
                        package_id,
                    });
                }

                // 2. Resolve Cache Topology
                let cache_root = path.join("cache");
                if cache_root.is_dir() {
                    if let Ok(indices) = fs::read_dir(cache_root) {
                        for idx_entry in indices.filter_map(|e| e.ok()) {
                            let idx_path = idx_entry.path();
                            if !idx_path.is_dir() {
                                continue;
                            }

                            let level: u32 = fs::read_to_string(idx_path.join("level"))
                                .unwrap_or_default()
                                .trim()
                                .parse()
                                .unwrap_or(0);
                            if level == 0 {
                                continue;
                            }

                            let size = fs::read_to_string(idx_path.join("size"))
                                .unwrap_or_default()
                                .trim()
                                .to_string();
                            let type_name = fs::read_to_string(idx_path.join("type"))
                                .unwrap_or_default()
                                .trim()
                                .to_string();

                            let shared_cpus = fs::read_to_string(idx_path.join("shared_cpu_list"))
                                .or_else(|_| fs::read_to_string(idx_path.join("shared_cpus")))
                                .unwrap_or_default()
                                .trim()
                                .to_string();

                            let cache_id = format!("{}-{}", level, shared_cpus);
                            if !seen_caches.contains(&cache_id) {
                                seen_caches.insert(cache_id);
                                cache_blocks.push(CacheBlock {
                                    level,
                                    size,
                                    type_name,
                                    shared_cpus,
                                });
                            }
                        }
                    }
                }
            }
        }

        // 3. Resolve NUMA Memory
        if let Ok(node_dir) = fs::read_dir("/sys/devices/system/node") {
            for entry in node_dir.filter_map(|e| e.ok()) {
                let name = entry.file_name().into_string().unwrap_or_default();
                if name.starts_with("node") && name[4..].chars().all(|c| c.is_numeric()) {
                    let node_id: u32 = name[4..].parse().unwrap_or(0);
                    let meminfo =
                        fs::read_to_string(entry.path().join("meminfo")).unwrap_or_default();

                    let mut total = 0;
                    let mut free = 0;
                    let mut inactive = 0;
                    let mut kreclaimable = 0;
                    let mut sreclaimable = 0;

                    for line in meminfo.lines() {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() < 4 {
                            continue;
                        }
                        let val = parts[3].parse::<u64>().unwrap_or(0);

                        if line.contains("MemTotal:") {
                            total = val;
                        } else if line.contains("MemFree:") {
                            free = val;
                        } else if line.contains("Inactive:") {
                            inactive = val;
                        } else if line.contains("KReclaimable:") {
                            kreclaimable = val;
                        } else if line.contains("SReclaimable:") {
                            sreclaimable = val;
                        }
                    }

                    // Available = Free + Inactive (Page Cache) + Reclaimable Kernel Memory
                    let reclaimable_kernel = if kreclaimable > 0 {
                        kreclaimable
                    } else {
                        sreclaimable
                    };
                    let available = free + inactive + reclaimable_kernel;

                    numa_nodes.push(NumaNode {
                        id: node_id,
                        total_kb: total,
                        free_kb: available,
                    });
                }
            }
        }

        cores.sort_by_key(|c| c.logical_id);
        Ok(SystemTopology {
            cores,
            cache_blocks,
            numa_nodes,
        })
    }
}
