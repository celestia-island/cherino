use std::path::PathBuf;
use std::time::Duration;

use super::{
    errors::{ContainerError, ContainerResult},
    manager::ContainerManager,
    types::{ContainerVersion, DockerDiskUsage},
};

use bollard::models::{SystemInfo, SystemVersion};

/// The per-call bound for the daemon version read. Generous enough for a
/// busy daemon's cold cache, well inside any caller's own row window.
pub const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// `GET /version` under a per-call bound; the timeout is a parameter so
/// tests can pin the bound against a silent daemon without waiting out
/// the production constant. Extracted as a free function over `&Docker`
/// so the test drives the PRODUCTION path, not a copy of it.
pub(crate) async fn version_with_timeout(
    docker: &bollard::Docker,
    timeout: Duration,
) -> ContainerResult<Option<ContainerVersion>> {
    let version = tokio::time::timeout(timeout, docker.version())
        .await
        .map_err(|_| {
            ContainerError::Connection(format!("daemon version read timed out after {timeout:?}"))
        })??;
    Ok(map_version(&version))
}

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

/// Map a daemon `/version` payload to the version a consumer renders after
/// the product name. The daemon's OWN `Version` is the product version; its
/// `ApiVersion` is a qualifier and rides in `tags` (a tag in the version slot
/// would read as part of the product version). An absent or blank `Version`
/// maps to `None` rather than an empty string — the panel must render a bare
/// product name, never `containers:`. Extracted as a pure function so the
/// tests pin the PRODUCTION mapping, not a copy of it.
fn map_version(version: &SystemVersion) -> Option<ContainerVersion> {
    let own = version.version.as_deref()?.trim();
    if own.is_empty() {
        return None;
    }
    let mut mapped = ContainerVersion::new(own);
    if let Some(api) = version
        .api_version
        .as_deref()
        .map(str::trim)
        .filter(|api| !api.is_empty())
    {
        mapped.tags.push(format!("api {api}"));
    }
    Some(mapped)
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
        let bucket_field = |total: Option<i64>| total.unwrap_or(0).max(0) as u64;
        let bucket_total = |u: Option<&bollard::models::ImagesDiskUsage>| {
            bucket_field(u.and_then(|b| b.total_size))
        };
        let bucket_reclaimable = |u: Option<&bollard::models::ImagesDiskUsage>| {
            bucket_field(u.and_then(|b| b.reclaimable))
        };
        DockerDiskUsage {
            layers_bytes: bucket_total(usage.image_usage.as_ref()),
            volumes_bytes: bucket_field(usage.volume_usage.as_ref().and_then(|u| u.total_size)),
            containers_rw_bytes: bucket_field(
                usage.container_usage.as_ref().and_then(|u| u.total_size),
            ),
            build_cache_bytes: bucket_field(
                usage.build_cache_usage.as_ref().and_then(|u| u.total_size),
            ),
            reclaimable_bytes: bucket_reclaimable(usage.image_usage.as_ref())
                + bucket_field(usage.volume_usage.as_ref().and_then(|u| u.reclaimable))
                + bucket_field(usage.container_usage.as_ref().and_then(|u| u.reclaimable))
                + bucket_field(usage.build_cache_usage.as_ref().and_then(|u| u.reclaimable)),
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

    /// The daemon's own version (`GET /version`), split into the product
    /// version and its qualifiers — the `containers` dependency row's
    /// `产品名:产品版本` identity.
    ///
    /// The read carries a per-call bound (`VERSION_TIMEOUT`): the other
    /// daemon reads here (`storage_root`, `docker_disk_usage`) are also
    /// unbounded, but those callers own a timeout of their own, while the
    /// version read feeds a probe row whose entire allowance is a few
    /// seconds — an unbounded leg there would let a wedged daemon hold a
    /// whole health answer hostage. A timed-out read is a transport
    /// failure (`Connection`), never a silent `None`: "answered but
    /// failed" stays an error by the same contract as every other read.
    pub async fn version(&self) -> ContainerResult<Option<ContainerVersion>> {
        version_with_timeout(&self.docker, self.version_bound).await
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

    /// The `/version` mapping: daemon version in the version slot, API
    /// version after it as a tag.
    #[test]
    fn reported_version_carries_the_daemon_version_and_its_api_tag() {
        let version = SystemVersion {
            version: Some("27.3.1".to_string()),
            api_version: Some("1.47".to_string()),
            ..Default::default()
        };
        let mapped = map_version(&version).expect("a reported version maps");
        assert_eq!(mapped.version, "27.3.1");
        assert_eq!(mapped.tags, vec!["api 1.47".to_string()]);
    }

    #[test]
    fn a_daemon_without_an_api_version_adds_no_tag() {
        let version = SystemVersion {
            version: Some("27.3.1".to_string()),
            api_version: None,
            ..Default::default()
        };
        let mapped = map_version(&version).expect("a reported version maps");
        assert_eq!(mapped.version, "27.3.1");
        assert!(mapped.tags.is_empty(), "no api version → no tag");
        // A blank api version is the same as none: an `api ` tag with no
        // number would be a label, not a qualifier.
        let blank = SystemVersion {
            version: Some("27.3.1".to_string()),
            api_version: Some("  ".to_string()),
            ..Default::default()
        };
        assert!(map_version(&blank).expect("maps").tags.is_empty());
    }

    /// Both slots are trimmed before they reach a consumer: a daemon (or a
    /// proxy rewriting its answer) that pads the field must still render as
    /// `containers:27.3.1 api 1.47`, not with the padding visible.
    #[test]
    fn padded_version_and_api_version_are_trimmed() {
        let version = SystemVersion {
            version: Some("  27.3.1  ".to_string()),
            api_version: Some(" 1.47 ".to_string()),
            ..Default::default()
        };
        let mapped = map_version(&version).expect("a reported version maps");
        assert_eq!(mapped.version, "27.3.1");
        assert_eq!(mapped.tags, vec!["api 1.47".to_string()]);
    }

    /// No version from the daemon is `None`, never an empty string: the
    /// consumer's version slot must stay empty rather than render
    /// `containers:`.
    #[test]
    fn a_silent_daemon_maps_to_no_version() {
        for silent in [
            SystemVersion::default(),
            SystemVersion {
                version: Some(String::new()),
                ..Default::default()
            },
            SystemVersion {
                version: Some("   ".to_string()),
                ..Default::default()
            },
        ] {
            assert!(
                map_version(&silent).is_none(),
                "a daemon that names no version must map to None"
            );
        }
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
                reclaimable: Some(100),
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
        assert_eq!(summed.reclaimable_bytes, 100);
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
