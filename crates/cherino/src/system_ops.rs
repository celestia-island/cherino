use std::path::PathBuf;

use super::{
    errors::{ContainerError, ContainerResult},
    manager::ContainerManager,
    types::DockerDiskUsage,
};

use bollard::models::SystemInfo;

/// Map a daemon `/info` payload to the storage root. A daemon that omits
/// `DockerRootDir` — or reports it empty (defensive: the API allows an
/// empty string, and `PathBuf("")` would resolve to the caller's CWD) —
/// must surface a `Connection` failure, never an empty path a consumer
/// could mistake for `/`. Extracted as a pure function so the tests pin
/// the PRODUCTION mapping, not a copy of it.
fn map_root_dir(info: &SystemInfo) -> ContainerResult<PathBuf> {
    match info.docker_root_dir.as_deref() {
        Some(dir) if !dir.is_empty() => Ok(PathBuf::from(dir)),
        _ => Err(ContainerError::Connection(
            "dockerd did not report docker_root_dir in /info".to_string(),
        )),
    }
}

impl ContainerManager {
    /// Sum the daemon's `/system/df` payload into the fixed usage
    /// buckets. The typed response carries each bucket's OWN
    /// deduplicated `total_size` — the daemon's authoritative figure for
    /// that bucket; summing the per-item lists here would double-count
    /// shared layers. Pure function over the response so tests pin the
    /// PRODUCTION sums (missing buckets are honest zeros; negative
    /// values — the API permits -1 "not available" on some drivers —
    /// saturate to zero rather than poisoning the totals).
    pub(crate) fn sum_disk_usage(
        usage: &bollard::models::SystemDataUsageResponse,
    ) -> DockerDiskUsage {
        let bucket_total = |total: Option<i64>| total.unwrap_or(0).max(0) as u64;
        DockerDiskUsage {
            layers_bytes: bucket_total(usage.image_usage.as_ref().and_then(|u| u.total_size)),
            volumes_bytes: bucket_total(usage.volume_usage.as_ref().and_then(|u| u.total_size)),
            containers_rw_bytes: bucket_total(
                usage.container_usage.as_ref().and_then(|u| u.total_size),
            ),
            build_cache_bytes: bucket_total(
                usage.build_cache_usage.as_ref().and_then(|u| u.total_size),
            ),
        }
    }

    /// The daemon's own disk accounting for its storage root — the
    /// honest allocated figure for a data-root the caller cannot
    /// traverse (root-owned path, engine in a container): dockerd
    /// computes it from the real on-disk layers, volumes and container
    /// rw deltas, not from logical sizes.
    pub async fn disk_usage(&self) -> ContainerResult<DockerDiskUsage> {
        let usage = self.docker.df(None).await?;
        Ok(Self::sum_disk_usage(&usage))
    }

    /// The dockerd data-root (`DockerRootDir` from `/info` — overlay2,
    /// containers, volumes metadata all live under it). Storage hygiene
    /// consumers anchor their usage probes and trims here.
    ///
    /// The daemon reports the path but does not promise the caller's mount
    /// namespace can see it; callers must degrade explicitly when the
    /// filesystem is not reachable (never guess substitute paths).
    pub async fn storage_root(&self) -> ContainerResult<PathBuf> {
        let info = self.docker.info().await?;
        map_root_dir(&info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mapping is the whole contract on the Docker arm; a
    /// live-daemon round-trip belongs to deployment-side acceptance, not
    /// unit tests. These call the PRODUCTION pure function.
    #[test]
    fn reported_root_dir_becomes_the_storage_root() {
        let info = SystemInfo {
            docker_root_dir: Some("/var/lib/docker".to_string()),
            ..Default::default()
        };
        assert_eq!(
            map_root_dir(&info).expect("a reported path maps"),
            PathBuf::from("/var/lib/docker")
        );
    }

    #[test]
    fn missing_root_dir_maps_to_connection_error() {
        let info = SystemInfo {
            docker_root_dir: None,
            ..Default::default()
        };
        let err = map_root_dir(&info).expect_err("None root dir must error");
        assert!(
            matches!(err, ContainerError::Connection(_)),
            "unexpected error shape: {err}"
        );
        assert!(err.to_string().contains("docker_root_dir"));
    }

    #[test]
    fn empty_root_dir_maps_to_connection_error() {
        // The API permits an empty string; passing it through would hand
        // `PathBuf("")` to consumers, which resolves to their CWD.
        let info = SystemInfo {
            docker_root_dir: Some(String::new()),
            ..Default::default()
        };
        assert!(matches!(
            map_root_dir(&info),
            Err(ContainerError::Connection(_))
        ));
    }

    #[test]
    fn disk_usage_sums_present_buckets_and_saturates_negatives() {
        use bollard::models::{
            BuildCacheDiskUsage, ContainersDiskUsage, ImagesDiskUsage, SystemDataUsageResponse,
            VolumesDiskUsage,
        };
        let usage = SystemDataUsageResponse {
            image_usage: Some(ImagesDiskUsage {
                total_size: Some(1000),
                ..Default::default()
            }),
            volume_usage: Some(VolumesDiskUsage {
                total_size: Some(200),
                ..Default::default()
            }),
            container_usage: Some(ContainersDiskUsage {
                total_size: Some(50),
                ..Default::default()
            }),
            build_cache_usage: Some(BuildCacheDiskUsage {
                total_size: Some(25),
                ..Default::default()
            }),
        };
        let summed = ContainerManager::sum_disk_usage(&usage);
        assert_eq!(summed.layers_bytes, 1000);
        assert_eq!(summed.volumes_bytes, 200);
        assert_eq!(summed.containers_rw_bytes, 50);
        assert_eq!(summed.build_cache_bytes, 25);
        assert_eq!(summed.total_bytes(), 1275);
    }

    #[test]
    fn disk_usage_negative_bucket_saturates_to_zero() {
        use bollard::models::{
            BuildCacheDiskUsage, ContainersDiskUsage, ImagesDiskUsage, SystemDataUsageResponse,
            VolumesDiskUsage,
        };
        // All four buckets fed -1: the saturation lives in the SHARED
        // bucket_total closure, so every arm must prove it (a future arm
        // hand-rolled around the closure would otherwise survive).
        let usage = SystemDataUsageResponse {
            image_usage: Some(ImagesDiskUsage {
                total_size: Some(-1),
                ..Default::default()
            }),
            volume_usage: Some(VolumesDiskUsage {
                total_size: Some(-1),
                ..Default::default()
            }),
            container_usage: Some(ContainersDiskUsage {
                total_size: Some(-1),
                ..Default::default()
            }),
            build_cache_usage: Some(BuildCacheDiskUsage {
                total_size: Some(-1),
                ..Default::default()
            }),
        };
        let summed = ContainerManager::sum_disk_usage(&usage);
        assert_eq!(
            summed,
            DockerDiskUsage::default(),
            "every -1 bucket must saturate to 0, not poison any total"
        );
    }

    #[test]
    fn disk_usage_empty_response_is_all_zeros() {
        use bollard::models::SystemDataUsageResponse;
        let summed = ContainerManager::sum_disk_usage(&SystemDataUsageResponse::default());
        assert_eq!(summed, DockerDiskUsage::default());
        assert_eq!(summed.total_bytes(), 0);
    }
}
