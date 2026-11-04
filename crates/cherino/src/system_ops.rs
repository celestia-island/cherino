use std::path::PathBuf;

use super::{
    errors::{ContainerError, ContainerResult},
    manager::ContainerManager,
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
}
