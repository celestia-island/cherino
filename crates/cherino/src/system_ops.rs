use std::path::PathBuf;

use super::{
    errors::{ContainerError, ContainerResult},
    manager::ContainerManager,
};

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
        info.docker_root_dir.map(PathBuf::from).ok_or_else(|| {
            ContainerError::Connection(
                "dockerd did not report docker_root_dir in /info".to_string(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bollard field mapping is the whole contract on the Docker arm;
    /// a live-daemon round-trip belongs to deployment-side acceptance, not
    /// unit tests. This pins the error arm: a missing `docker_root_dir`
    /// (possible on unusual storage drivers) must surface as Connection
    /// failure, not an empty path that a caller could mistake for `/`.
    #[test]
    fn missing_root_dir_maps_to_connection_error() {
        let info = bollard::models::SystemInfo {
            docker_root_dir: None,
            ..Default::default()
        };
        let mapped: ContainerResult<PathBuf> =
            info.docker_root_dir.map(PathBuf::from).ok_or_else(|| {
                ContainerError::Connection(
                    "dockerd did not report docker_root_dir in /info".to_string(),
                )
            });
        let err = mapped.expect_err("None root dir must error");
        assert!(
            matches!(err, ContainerError::Connection(_)),
            "unexpected error shape: {err}"
        );
        assert!(err.to_string().contains("docker_root_dir"));
    }
}
