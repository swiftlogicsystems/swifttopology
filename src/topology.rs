use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

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
    pub shared_cpus: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct SystemTopology {
    pub cores: Vec<CpuCore>,
    pub cache_blocks: Vec<CacheBlock>,
}

impl SystemTopology {
    pub fn resolve() -> Result<Self> {
        let mut cores = Vec::new();
        let cpu_dir = fs::read_dir("/sys/devices/system/cpu")?;

        for entry in cpu_dir {
            let entry = entry?;
            let name = entry.file_name().into_string().unwrap_or_default();
            if name.starts_with("cpu") && name[3..].chars().all(|c| c.is_numeric()) {
                let id: u32 = name[3..].parse()?;
                let topo_path = entry.path().join("topology");

                if topo_path.exists() {
                    let package_id = fs::read_to_string(topo_path.join("physical_package_id"))?
                        .trim()
                        .parse()?;
                    let core_id = fs::read_to_string(topo_path.join("core_id"))?
                        .trim()
                        .parse()?;

                    cores.push(CpuCore {
                        logical_id: id,
                        physical_id: core_id,
                        package_id,
                    });
                }
            }
        }
        //Sort by logical_id
        cores.sort_by_key(|c| c.logical_id);
        Ok(SystemTopology {
            cores,
            caches: Vec::new(),
        })
    }
}
