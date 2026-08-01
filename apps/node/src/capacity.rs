use std::path::Path;

use anyhow::Context;
use grass_node_protocol::NodeResources;
use sysinfo::{Disks, System};

use crate::config::ServeConfig;

const BYTES_PER_MB: u64 = 1024 * 1024;

pub fn detected_capacity_from(
    logical_cpus: u64,
    memory_mb: u64,
    available_disk_mb: u64,
) -> NodeResources {
    NodeResources {
        cpu_millicores: logical_cpus.saturating_mul(800).max(1),
        memory_mb: memory_mb
            .saturating_mul(75)
            .checked_div(100)
            .unwrap_or(0)
            .max(1),
        disk_mb: available_disk_mb
            .saturating_mul(80)
            .checked_div(100)
            .unwrap_or(0)
            .max(1),
        max_deployments: 10,
    }
}

pub fn detect(config: &ServeConfig) -> anyhow::Result<NodeResources> {
    std::fs::create_dir_all(&config.artifact_cache_root).with_context(|| {
        format!(
            "failed to create artifact cache root {}",
            config.artifact_cache_root
        )
    })?;
    let cache_root = std::fs::canonicalize(&config.artifact_cache_root).with_context(|| {
        format!(
            "failed to resolve artifact cache root {}",
            config.artifact_cache_root
        )
    })?;

    let system = System::new_all();
    let logical_cpus = u64::try_from(
        std::thread::available_parallelism()
            .context("failed to detect logical CPU count")?
            .get(),
    )
    .unwrap_or(u64::MAX);
    let memory_mb = system.total_memory().checked_div(BYTES_PER_MB).unwrap_or(0);

    let disks = Disks::new_with_refreshed_list();
    let disk = disks
        .list()
        .iter()
        .filter(|disk| cache_root.starts_with(disk.mount_point()))
        .max_by_key(|disk| mount_depth(disk.mount_point()))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "failed to find the filesystem containing {}",
                cache_root.display()
            )
        })?;
    let available_disk_mb = disk
        .available_space()
        .checked_div(BYTES_PER_MB)
        .unwrap_or(0);
    let detected = detected_capacity_from(logical_cpus, memory_mb, available_disk_mb);
    let configured = &config.capacity;

    Ok(NodeResources {
        cpu_millicores: nonzero_or(configured.cpu_millicores, detected.cpu_millicores),
        memory_mb: nonzero_or(configured.memory_mb, detected.memory_mb),
        disk_mb: nonzero_or(configured.disk_mb, detected.disk_mb),
        max_deployments: nonzero_or(configured.max_deployments, detected.max_deployments),
    })
}

fn mount_depth(path: &Path) -> usize {
    path.components().count()
}

fn nonzero_or<T>(configured: T, detected: T) -> T
where
    T: Copy + Default + PartialEq,
{
    if configured == T::default() {
        detected
    } else {
        configured
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detected_capacity_applies_scheduling_percentages() {
        let detected = detected_capacity_from(4, 2_048, 10_000);

        assert_eq!(detected.cpu_millicores, 3_200);
        assert_eq!(detected.memory_mb, 1_536);
        assert_eq!(detected.disk_mb, 8_000);
        assert_eq!(detected.max_deployments, 10);
    }
}
