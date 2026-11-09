use std::{collections::HashMap, path::Path};

use async_trait::async_trait;

use super::{
    errors::{ContainerError, ContainerResult},
    types::{
        ContainerCreateParams, ContainerDetail, ContainerForkParams, ContainerInfo,
        ContainerVersion, ExecOutput, ImageInfo, PathChange, WritableRootfs,
    },
};

#[async_trait]
pub trait ContainerOps: Send + Sync {
    // -- Lifecycle --

    async fn create(&self, params: &ContainerCreateParams) -> ContainerResult<ContainerInfo>;
    async fn start(&self, container_id: &str) -> ContainerResult<()>;
    async fn stop(&self, container_id: &str) -> ContainerResult<()>;
    async fn remove(&self, container_id: &str, force: bool) -> ContainerResult<()>;
    async fn restart(&self, container_id: &str) -> ContainerResult<()>;

    // -- Query --

    async fn list(&self) -> ContainerResult<Vec<ContainerInfo>>;
    async fn list_with_filter(
        &self,
        name_prefix: Option<&str>,
        label_filter: Option<HashMap<String, String>>,
        all: bool,
    ) -> ContainerResult<Vec<ContainerInfo>>;
    async fn inspect(&self, container_id: &str) -> ContainerResult<ContainerDetail>;
    async fn is_running(&self, container_id: &str) -> ContainerResult<bool>;

    // -- Exec --

    async fn exec(&self, container_id: &str, command: &[&str]) -> ContainerResult<ExecOutput>;

    // -- Fork / Commit --

    async fn fork(&self, params: &ContainerForkParams) -> ContainerResult<ContainerInfo>;
    async fn commit(
        &self,
        container_id: &str,
        repo: &str,
        tag: Option<&str>,
    ) -> ContainerResult<String>;
    async fn commit_with_labels(
        &self,
        container_id: &str,
        repo: &str,
        tag: Option<&str>,
        labels: Option<&HashMap<String, String>>,
    ) -> ContainerResult<String>;

    // -- Health / Ensure --

    async fn wait_healthy(
        &self,
        container_id: &str,
        timeout: std::time::Duration,
    ) -> ContainerResult<()>;
    async fn ensure_running(&self, container_id: &str) -> ContainerResult<ContainerInfo>;
    async fn recreate(
        &self,
        container_name: &str,
        new_image: &str,
    ) -> ContainerResult<ContainerInfo>;

    // -- Images --

    async fn list_images(&self) -> ContainerResult<Vec<ImageInfo>>;
    async fn pull_image(&self, image: &str) -> ContainerResult<String>;
    async fn image_exists(&self, image: &str) -> ContainerResult<bool>;
    async fn remove_image(&self, image: &str, force: bool) -> ContainerResult<()>;
    async fn remove_with_image(
        &self,
        container_id: &str,
        image: &str,
        force: bool,
    ) -> ContainerResult<()>;

    // -- Volumes --

    async fn create_volume(&self, name: &str) -> ContainerResult<String>;
    async fn remove_volume(&self, name: &str, force: bool) -> ContainerResult<()>;
    async fn volume_exists(&self, name: &str) -> ContainerResult<bool>;
    async fn list_volumes(&self) -> ContainerResult<Vec<super::types::DockerVolumeInfo>>;

    // -- Identity --

    /// The backend's SELF-REPORTED version, split the way consumers render
    /// `产品名:产品版本`: the product's own version plus qualifiers that ride
    /// after it as tags (a docker daemon's API version, say).
    ///
    /// Default: `Ok(None)` — "this backend cannot name its version". That is
    /// deliberately NOT [`ContainerError::UnsupportedBackend`] the way
    /// [`Self::storage_root`] is: a version nobody can name is simply
    /// unnameable (the consumer renders the bare product name), while a
    /// fabricated storage path would be dangerous. A backend that CAN answer
    /// but fails to (daemon down, permission denied) still returns an error —
    /// the difference between "no version" and "could not read one" is what
    /// consumers log. One corner is deliberately NOT an error: a backend that
    /// answers but carries no version in its answer (a 200 payload whose
    /// version field is absent or blank) reports `Ok(None)` — the row
    /// degrades to the bare product name, where an `Err` would log a probe
    /// failure for a backend that is answering fine.
    ///
    /// Presence is therefore PLATFORM- and arm-dependent, not type-wide: the
    /// Linux youki backend answers; its non-Linux stub (`cherino-runtime`)
    /// inherits the default and stays silent; docker answers when the daemon
    /// reports a version and stays silent when the payload carries none; and
    /// the CLI adapters answer by probing `<binary> --version` (unparsable
    /// output degrades to the silent default). Consumers must treat `None`
    /// as "not named here", never as "this platform has no containers".
    async fn version(&self) -> ContainerResult<Option<ContainerVersion>> {
        Ok(None)
    }

    // -- Storage Topology --

    /// The backend's storage root: the directory under which this backend
    /// keeps its ON-DISK writable state (Docker's data-root — overlay2 /
    /// containers / volumes; the youki data dir — rootfs cache and
    /// container trees). Storage hygiene consumers use it as the
    /// usage-probe and trim anchor path.
    ///
    /// Coverage note: youki named volumes deliberately sit on the RUN
    /// dir (tmpfs) and are NOT part of this root — a usage probe
    /// anchored here sees no tmpfs volume bytes, which is the honest
    /// number for reclaim governance (tmpfs holds no disk debt).
    ///
    /// Default: [`ContainerError::UnsupportedBackend`] — backends opt in.
    /// The path may be invisible to the caller's mount namespace (an
    /// engine in a container cannot see the host's data-root); consumers
    /// must degrade explicitly rather than guessing substitute paths.
    async fn storage_root(&self) -> ContainerResult<std::path::PathBuf> {
        Err(ContainerError::UnsupportedBackend(
            "storage_root is not implemented by this backend".to_string(),
        ))
    }

    /// The daemon's own disk accounting for its storage root
    /// (`GET /system/df`, summed into fixed buckets) — the honest
    /// allocated figure for a data-root the caller cannot traverse.
    /// Missing buckets are honest zeros: a total is a LOWER BOUND on the
    /// daemon's real usage, never a measurement of emptiness. Buckets
    /// are disjoint by the daemon's construction (containers carry only
    /// their writable deltas; image layers are deduplicated), so
    /// [`DockerDiskUsage::total_bytes`] does not double-count.
    /// Default: [`ContainerError::UnsupportedBackend`] — backends opt in.
    async fn docker_disk_usage(&self) -> ContainerResult<super::types::DockerDiskUsage> {
        Err(ContainerError::UnsupportedBackend(
            "docker_disk_usage is not implemented by this backend".to_string(),
        ))
    }

    // -- Logs --

    async fn logs(&self, container_id: &str, tail: usize) -> ContainerResult<Vec<String>>;

    async fn get_container_logs(&self, container_id: &str, tail: usize) -> ContainerResult<String>;

    // -- Server Detection --

    async fn detect_server_status(&self, container_name: &str) -> super::types::ServerStatus {
        let containers = self
            .list_with_filter(Some(&format!("^{}$", container_name)), None, false)
            .await;
        match containers {
            Ok(c) if !c.is_empty() => super::types::ServerStatus::Running,
            Ok(_) => {
                let all = self
                    .list_with_filter(Some(&format!("^{}$", container_name)), None, true)
                    .await;
                match all {
                    Ok(c) if !c.is_empty() => super::types::ServerStatus::Stopped,
                    Ok(_) => super::types::ServerStatus::NotExists,
                    Err(_) => super::types::ServerStatus::Unknown,
                }
            }
            Err(_) => super::types::ServerStatus::Unknown,
        }
    }

    // -- Rootfs Access (merge pipeline replacement) --

    async fn writable_rootfs(&self, container_id: &str) -> ContainerResult<WritableRootfs>;

    async fn diff_workspace(
        &self,
        container_id: &str,
        workspace_path: &Path,
        base_path: &str,
    ) -> ContainerResult<Vec<PathChange>>;

    async fn download_archive(&self, container_id: &str, path: &str) -> ContainerResult<Vec<u8>>;

    async fn upload_archive(
        &self,
        container_id: &str,
        path: &str,
        data: Vec<u8>,
    ) -> ContainerResult<()>;

    // -- Clone --

    fn clone_boxed(&self) -> Box<dyn ContainerOps>;
}

impl Clone for Box<dyn ContainerOps> {
    fn clone(&self) -> Self {
        self.clone_boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend that implements NOTHING: every trait method inherits its
    /// default or errors out. Its only job is pinning default-method arms
    /// (storage_root, docker_disk_usage) and the version default (version →
    /// `None`) so the trait contract stays explicit — a backend that never
    /// opted in must surface Unsupported or "cannot name itself", never a
    /// path, number or version string a consumer could mistake for real.
    #[derive(Debug, Clone, Copy, Default)]
    struct NullBackend;

    macro_rules! unimplemented_arm {
        ($($method:ident($($arg:ident: $ty:ty),*) -> $ret:ty),* $(,)?) => {
            #[async_trait::async_trait]
            impl ContainerOps for NullBackend {
                $(async fn $method(&self, $(_: $ty),*) -> $ret {
                    Err(ContainerError::UnsupportedBackend(stringify!($method).to_string()))
                })*
                fn clone_boxed(&self) -> Box<dyn ContainerOps> {
                    Box::new(*self)
                }
            }
        };
    }

    unimplemented_arm! {
        create(_p: &ContainerCreateParams) -> ContainerResult<ContainerInfo>,
        start(_id: &str) -> ContainerResult<()>,
        stop(_id: &str) -> ContainerResult<()>,
        remove(_id: &str, _force: bool) -> ContainerResult<()>,
        restart(_id: &str) -> ContainerResult<()>,
        list() -> ContainerResult<Vec<ContainerInfo>>,
        list_with_filter(_p: Option<&str>, _l: Option<HashMap<String, String>>, _all: bool) -> ContainerResult<Vec<ContainerInfo>>,
        inspect(_id: &str) -> ContainerResult<ContainerDetail>,
        is_running(_id: &str) -> ContainerResult<bool>,
        exec(_id: &str, _cmd: &[&str]) -> ContainerResult<ExecOutput>,
        fork(_p: &ContainerForkParams) -> ContainerResult<ContainerInfo>,
        commit(_id: &str, _r: &str, _t: Option<&str>) -> ContainerResult<String>,
        commit_with_labels(_id: &str, _r: &str, _t: Option<&str>, _l: Option<&HashMap<String, String>>) -> ContainerResult<String>,
        wait_healthy(_id: &str, _t: std::time::Duration) -> ContainerResult<()>,
        ensure_running(_id: &str) -> ContainerResult<ContainerInfo>,
        recreate(_n: &str, _i: &str) -> ContainerResult<ContainerInfo>,
        list_images() -> ContainerResult<Vec<ImageInfo>>,
        pull_image(_i: &str) -> ContainerResult<String>,
        image_exists(_i: &str) -> ContainerResult<bool>,
        remove_image(_i: &str, _f: bool) -> ContainerResult<()>,
        remove_with_image(_c: &str, _i: &str, _f: bool) -> ContainerResult<()>,
        create_volume(_n: &str) -> ContainerResult<String>,
        remove_volume(_n: &str, _f: bool) -> ContainerResult<()>,
        volume_exists(_n: &str) -> ContainerResult<bool>,
        list_volumes() -> ContainerResult<Vec<crate::types::DockerVolumeInfo>>,
        get_container_logs(_id: &str, _t: usize) -> ContainerResult<String>,
        writable_rootfs(_id: &str) -> ContainerResult<WritableRootfs>,
        diff_workspace(_c: &str, _w: &Path, _b: &str) -> ContainerResult<Vec<crate::types::PathChange>>,
        download_archive(_c: &str, _p: &str) -> ContainerResult<Vec<u8>>,
        upload_archive(_c: &str, _p: &str, _d: Vec<u8>) -> ContainerResult<()>,
        logs(_id: &str, _t: usize) -> ContainerResult<Vec<String>>
    }

    #[tokio::test]
    async fn storage_root_defaults_to_unsupported() {
        let err = NullBackend
            .storage_root()
            .await
            .expect_err("default arm must not fabricate a path");
        assert!(
            matches!(err, ContainerError::UnsupportedBackend(_)),
            "unexpected error shape: {err}"
        );
    }

    /// The version default arm is the third kind of "no fabricated
    /// answers": a backend that cannot name itself renders the bare
    /// product name, and `Ok(None)` is how it says so — never a
    /// placeholder version, never an error a consumer would log as a probe
    /// failure.
    #[tokio::test]
    async fn version_defaults_to_none() {
        let version = NullBackend
            .version()
            .await
            .expect("an unnameable version is not an error");
        assert!(version.is_none(), "no backend may inherit a version");
    }

    /// The docker_disk_usage default arm is the "no fabricated numbers"
    /// twin of the path contract: a backend that never opted in must not
    /// hand back a zero-filled usage struct a consumer could read as an
    /// empty data-root.
    #[tokio::test]
    async fn docker_disk_usage_defaults_to_unsupported() {
        let err = NullBackend
            .docker_disk_usage()
            .await
            .expect_err("default arm must not fabricate zeros");
        assert!(
            matches!(err, ContainerError::UnsupportedBackend(_)),
            "unexpected error shape: {err}"
        );
    }
}
