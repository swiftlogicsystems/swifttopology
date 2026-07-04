use anyhow::{Context, Result};
use std::collections::HashSet; // Fixed: Added missing import
use std::fs;

#[derive(Debug, Clone)]
pub struct CpuCore {
    pub logical_id: u32,
    pub physical_id: u32,
    pub package_id: u32,
}

#[derive(Debug, Clone)]
pub struct CacheBlock {
    pub level: u32, // Changed to u32 to match expected types
    pub size: String,
    pub type_name: String,   // Added field to struct
    pub shared_cpus: String, // Named this 'shared_cpus' to keep it concise
}

#[derive(Debug, Clone)]
pub struct SystemTopology {
    pub cores: Vec<CpuCore>,
    pub cache_blocks: Vec<CacheBlock>,
}

impl SystemTopology {
    pub fn resolve() -> Result<Self> {
        let mut cores = Vec::new();
        let mut cache_blocks = Vec::new();
        let mut seen_caches = HashSet::new();

        let cpu_entries = fs::read_dir("/sys/devices/system/cpu")?
            .filter_map(|e| e.ok())
            .collect::<Vec<_>>();

        for entry in &cpu_entries {
            let name = entry.file_name().into_string().unwrap_or_default();

            if name.starts_with("cpu") && name[3..].chars().all(|c| c.is_numeric()) {
                let id: u32 = name[3..].parse()?;
                let path = entry.path();

                // 1. Resolve Core Topology
                let topo_path = path.join("topology");
                if topo_path.exists() {
                    let package_id = fs::read_to_string(topo_path.join("physical_package_id"))
                        .unwrap_or_else(|_| "0".to_string())
                        .trim()
                        .parse()
                        .unwrap_or(0);
                    let core_id = fs::read_to_string(topo_path.join("core_id"))
                        .unwrap_or_else(|_| "0".to_string())
                        .trim()
                        .parse()
                        .unwrap_or(0);

                    cores.push(CpuCore {
                        logical_id: id,
                        physical_id: core_id,
                        package_id,
                    });
                }

                // 2. Resolve Cache Topology
                let cache_dir = path.join("cache");
                if cache_dir.exists() {
                    if let Ok(indices) = fs::read_dir(cache_dir) {
                        for idx_entry in indices.filter_map(|e| e.ok()) {
                            let level: u32 = fs::read_to_string(idx_entry.path().join("level"))?
                                .trim()
                                .parse()?;
                            let size = fs::read_to_string(idx_entry.path().join("size"))?
                                .trim()
                                .to_string();
                            let type_name = fs::read_to_string(idx_entry.path().join("type"))?
                                .trim()
                                .to_string();
                            let shared_cpus =
                                fs::read_to_string(idx_entry.path().join("shared_cpu_list"))?
                                    .trim()
                                    .to_string();

                            let cache_id = format!("{}-{}", level, shared_cpus);
                            if !seen_caches.contains(&cache_id) {
                                seen_caches.insert(cache_id);
                                cache_blocks.push(CacheBlock {
                                    level,
                                    size,
                                    type_name,
                                    shared_cpus, // Corrected field name
                                });
                            }
                        }
                    }
                }
            }
        }

        cores.sort_by_key(|c| c.logical_id);

        Ok(SystemTopology {
            cores,
            cache_blocks,
        })
    }
}
