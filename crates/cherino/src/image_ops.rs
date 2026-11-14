use futures::StreamExt;
use std::collections::HashMap;
use uuid::Uuid;

use bollard::{
    container::LogOutput,
    models::FilesystemChange,
    query_parameters::{CommitContainerOptions, RemoveImageOptions, TagImageOptions},
};
use tracing::{info, warn};

use super::{
    errors::{ContainerError, ContainerResult},
    manager::ContainerManager,
    types::{ContainerForkParams, ContainerInfo, ForkOptions, ImageInfo, VolumeMount},
};

const COSMOS_FORK_PREFIX: &str = "cosmos-fork-";
const COSMOS_SNAPSHOT_PREFIX: &str = "cosmos-snapshot-";

/// Directories a runtime may back with tmpfs; reuses the sandbox preset's list so
/// there is one source of truth. Their *contents* never enter the writable layer,
/// so a change record naming the mount root is scaffolding. A record *below* one
/// is deliberately not scaffolding: seeing it proves the path is a real
/// directory, i.e. the container has genuine state there.
const RUNTIME_SCRATCH_ROOTS: [&str; 3] = super::security_profile::READONLY_SCRATCH_PATHS;

/// What the fast path needs to know about the source container.
struct SourceForkFacts {
    image_id: String,
    read_only_rootfs: Option<bool>,
}

/// True when a fork may start from the source container's own image instead of
/// committing a new one. All three preconditions must hold:
///
/// 1. the source rootfs is read-only, so no write can have reached the layer;
/// 2. every recorded change is runtime scaffolding — a bind-mount destination
///    from `volumes`, a tmpfs mount root, or an ancestor directory of a bind
///    destination (tmpfs roots are top-level, so they have no such ancestor);
/// 3. the source image id resolved, so there is an image to fork from.
///
/// Anything else (a real write, a writable rootfs, an unresolved image) returns
/// `false` and the caller takes the ordinary commit path. The response is a
/// normal `docker tag` of the source image under the fork tag, so
/// [`ContainerManager::remove_with_image`] still finds a `cosmos-fork-*` ref.
pub fn fork_can_reuse_source_image(
    changes: &[FilesystemChange],
    volumes: &[VolumeMount],
    source_read_only_rootfs: Option<bool>,
    source_image_id: Option<&str>,
) -> bool {
    if source_read_only_rootfs != Some(true) {
        return false;
    }
    let Some(image_id) = source_image_id else {
        return false;
    };
    if image_id.trim().is_empty() {
        return false;
    }
    changes.iter().all(|c| {
        // A delete means real content left the layer, so it is never scaffolding
        // even at a mount path (which a live mount makes undeletable anyway).
        !matches!(c.kind, bollard::models::ChangeType::_2) && is_runtime_scaffold(&c.path, volumes)
    })
}

/// A diff path that cannot carry container state: an exact tmpfs root, an exact
/// bind destination, or a strict ancestor directory of either.
fn is_runtime_scaffold(path: &str, volumes: &[VolumeMount]) -> bool {
    let path = path.trim_end_matches('/');
    if path.is_empty() || path == "/" {
        return false;
    }
    if RUNTIME_SCRATCH_ROOTS.contains(&path) {
        return true;
    }
    volumes.iter().any(|v| {
        let dest = v.container_path.trim_end_matches('/');
        !dest.is_empty() && (dest == path || dest.starts_with(&format!("{path}/")))
    })
}

/// `repo:tag` → (`repo`, `tag`); a bare repository resolves to `latest`, the
/// same default the commit path gets from Docker.
fn split_repo_tag(image_tag: &str) -> (String, String) {
    match image_tag.rsplit_once(':') {
        // A tag never contains '/', so `registry:5000/org/img` keeps its port in
        // the repository and resolves to `latest`.
        // A digest ref (`repo@sha256:...`) is not a tag either; leave it intact.
        Some((repo, tag))
            if !repo.is_empty() && !tag.is_empty() && !tag.contains('/') && !repo.contains('@') =>
        {
            (repo.to_string(), tag.to_string())
        }
        _ => (image_tag.to_string(), "latest".to_string()),
    }
}

impl ContainerManager {
    pub async fn remove_with_image(
        &self,
        container_id: &str,
        image: &str,
        force: bool,
    ) -> ContainerResult<()> {
        self.remove(container_id, force).await?;

        if image.starts_with(COSMOS_FORK_PREFIX) || image.starts_with(COSMOS_SNAPSHOT_PREFIX) {
            self.remove_image(image, true).await?;
        }

        Ok(())
    }

    pub async fn commit(
        &self,
        container_id: &str,
        repo: &str,
        tag: Option<&str>,
    ) -> ContainerResult<String> {
        self.commit_with_labels(container_id, repo, tag, None).await
    }

    pub async fn commit_with_labels(
        &self,
        container_id: &str,
        repo: &str,
        tag: Option<&str>,
        labels: Option<&std::collections::HashMap<String, String>>,
    ) -> ContainerResult<String> {
        let options = CommitContainerOptions {
            container: Some(container_id.to_string()),
            repo: Some(repo.to_string()),
            tag: Some(tag.map(|t| t.to_string()).unwrap_or_default()),
            ..Default::default()
        };

        let mut config = bollard::models::ContainerConfig::default();
        if let Some(lbls) = labels {
            config.labels = Some(lbls.clone());
        }

        let response = self
            .docker
            .commit_container(options, config)
            .await
            .map_err(|e| ContainerError::CommitFailed(e.to_string()))?;

        Ok(response.id)
    }

    /// Inspect the source container for the two facts the clean-layer fast path
    /// needs: its image id (to fork from) and whether its rootfs is read-only.
    async fn source_fork_facts(&self, container_id: &str) -> ContainerResult<SourceForkFacts> {
        let inspect = self
            .docker
            .inspect_container(container_id, None)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError {
                    status_code: 404, ..
                } => ContainerError::NotFound(container_id.to_string()),
                other => ContainerError::from(other),
            })?;

        Ok(SourceForkFacts {
            image_id: inspect.image.clone().unwrap_or_default(),
            read_only_rootfs: inspect.host_config.as_ref().and_then(|h| h.readonly_rootfs),
        })
    }

    /// Probe the source container and, when its layer is provably clean, tag its
    /// image under `image_tag` so the fork can start from that image.
    ///
    /// Best-effort by design: a probe failure (inspect 404, diff error, daemon
    /// hiccup, tag error) logs and answers `false`, i.e. the caller takes the
    /// ordinary commit path. This optimization must never turn a working fork
    /// into a failed one.
    async fn reuse_source_image_if_clean(
        &self,
        params: &ContainerForkParams,
        image_tag: &str,
    ) -> bool {
        let facts = match self.source_fork_facts(&params.source_container_id).await {
            Ok(facts) => facts,
            Err(e) => {
                warn!(
                    container = %params.source_container_id,
                    error = %e,
                    "fork: source inspect failed, taking the commit path"
                );
                return false;
            }
        };
        let changes = match self
            .container_filesystem_changes(&params.source_container_id)
            .await
        {
            Ok(changes) => changes,
            Err(e) => {
                warn!(
                    container = %params.source_container_id,
                    error = %e,
                    "fork: source diff failed, taking the commit path"
                );
                return false;
            }
        };
        if !fork_can_reuse_source_image(
            &changes,
            &params.volumes,
            facts.read_only_rootfs,
            Some(facts.image_id.as_str()),
        ) {
            return false;
        }

        let (repo, tag) = split_repo_tag(image_tag);
        let tag_options = TagImageOptions {
            repo: Some(repo),
            tag: Some(tag),
        };
        if let Err(e) = self
            .docker
            .tag_image(&facts.image_id, Some(tag_options))
            .await
        {
            warn!(
                container = %params.source_container_id,
                image = %image_tag,
                error = %e,
                "fork: tagging the clean source image failed, taking the commit path"
            );
            return false;
        }

        info!(
            container = %params.source_container_id,
            image = %image_tag,
            changes = changes.len(),
            "fork: source layer is clean, reusing its image instead of committing"
        );
        // A plain tag cannot carry labels; these are write-only audit metadata
        // today (no reader in cherino, scepter, neikos or fuuka).
        if params.commit_labels.as_ref().is_some_and(|l| !l.is_empty()) {
            info!(
                image = %image_tag,
                "fork: commit labels are not carried by the clean-layer fast path"
            );
        }
        true
    }

    pub async fn fork(&self, params: &ContainerForkParams) -> ContainerResult<ContainerInfo> {
        self.fork_with_options(params, ForkOptions::default()).await
    }

    /// [`Self::fork`] with explicit opt-in switches. `fork` is this call with
    /// [`ForkOptions::default`], so every existing caller keeps identical
    /// behaviour.
    pub async fn fork_with_options(
        &self,
        params: &ContainerForkParams,
        options: ForkOptions,
    ) -> ContainerResult<ContainerInfo> {
        let image_tag = params
            .image_tag
            .clone()
            .unwrap_or_else(|| format!("{}{}", COSMOS_FORK_PREFIX, Uuid::now_v7()));

        // U1 fast path (opt-in): when the source layer provably holds no state, a
        // commit would mint an image identical to the source image at ~0.95 s of
        // daemon work. Tag the source image under the fork tag instead so
        // `remove_with_image` keeps its `cosmos-fork-*` cleanup contract.
        let reuse_source_image =
            options.clean_layer && self.reuse_source_image_if_clean(params, &image_tag).await;

        if !reuse_source_image {
            self.commit_with_labels(
                &params.source_container_id,
                &image_tag,
                None,
                params.commit_labels.as_ref(),
            )
            .await
            .map_err(|e| {
                ContainerError::CommitFailed(format!(
                    "commit {} failed: {}",
                    params.source_container_id, e
                ))
            })?;
        }

        let create_params = super::types::ContainerCreateParams {
            name: params.new_name.clone(),
            image: image_tag.clone(),
            network: params.network.clone(),
            env: params.env.clone(),
            ports: Vec::new(),
            volumes: params.volumes.clone(),
            labels: params.labels.clone(),
            working_dir: None,
            log_driver: None,
            log_opts: HashMap::new(),
            healthcheck: None,
            user: params.user.clone(),
            memory_limit: params.memory_limit,
            nano_cpus: params.nano_cpus,
            pids_limit: params.pids_limit,
            cap_drop: params.cap_drop.clone(),
            cap_add: params.cap_add.clone(),
            security_opt: params.security_opt.clone(),
            read_only_rootfs: params.read_only_rootfs,
            egress_policy: None,
            group_add: None,
            devices: Vec::new(),
            compile_mode: false,
            egress_domain_allowlist_extra: Vec::new(),
        };

        match self.create(&create_params).await {
            Ok(info) => Ok(info),
            Err(e) => {
                // Clean up the image we minted to prevent disk space leaks. On the
                // reuse path the fork tag is an alias of the *source* image, so it
                // is untagged only (force=false): the daemon keeps the image record
                // any container still needs.
                let _ = self.remove_image(&image_tag, !reuse_source_image).await;
                Err(ContainerError::OperationFailed {
                    container_id: params.source_container_id.clone(),
                    message: format!("fork image is ready ({image_tag}), but create failed: {e}"),
                })
            }
        }
    }

    pub async fn remove_image(&self, image: &str, force: bool) -> ContainerResult<()> {
        let options = RemoveImageOptions {
            force,
            ..Default::default()
        };

        self.docker
            .remove_image(image, Some(options), None)
            .await
            .map_err(|e| ContainerError::ImageFailed(e.to_string()))?;

        Ok(())
    }

    pub async fn pull_image(&self, image: &str) -> ContainerResult<String> {
        use bollard::query_parameters::CreateImageOptions;

        let options = CreateImageOptions {
            from_image: Some(image.to_string()),
            ..Default::default()
        };

        let mut stream = self.docker.create_image(Some(options), None, None);
        let mut digest = String::new();

        while let Some(msg) = stream.next().await {
            match msg {
                Ok(info) => {
                    if let Some(id) = info.id {
                        digest = id;
                    }
                }
                Err(e) => {
                    return Err(ContainerError::ImageFailed(format!(
                        "pull {} failed: {}",
                        image, e
                    )));
                }
            }
        }

        info!(image = %image, digest = %digest, "image pulled");

        Ok(digest)
    }

    pub async fn inspect_image_id(&self, image: &str) -> ContainerResult<String> {
        let inspect = self
            .docker
            .inspect_image(image)
            .await
            .map_err(|e| ContainerError::ImageFailed(e.to_string()))?;

        Ok(inspect.id.unwrap_or_default())
    }

    pub async fn image_exists(&self, image: &str) -> ContainerResult<bool> {
        match self.docker.inspect_image(image).await {
            Ok(_) => Ok(true),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(false),
            Err(e) => Err(ContainerError::ImageFailed(e.to_string())),
        }
    }

    pub async fn list_images(&self) -> ContainerResult<Vec<ImageInfo>> {
        let options = bollard::query_parameters::ListImagesOptions {
            ..Default::default()
        };
        let images = self.docker.list_images(Some(options)).await?;
        let result = images
            .iter()
            .flat_map(|img| {
                if img.repo_tags.is_empty() {
                    vec![ImageInfo {
                        id: img.id.clone().trim_start_matches("sha256:").to_string(),
                        repository: "<none>".to_string(),
                        tag: "<none>".to_string(),
                        size: img.size,
                        created: img.created,
                    }]
                } else {
                    img.repo_tags
                        .iter()
                        .map(|tag_str: &String| {
                            let (repo, t) = tag_str
                                .rsplit_once(':')
                                .unwrap_or((tag_str.as_str(), "latest"));
                            ImageInfo {
                                id: img.id.clone().trim_start_matches("sha256:").to_string(),
                                repository: repo.to_string(),
                                tag: t.to_string(),
                                size: img.size,
                                created: img.created,
                            }
                        })
                        .collect()
                }
            })
            .collect();
        Ok(result)
    }

    pub async fn logs(&self, container_id: &str, tail: usize) -> ContainerResult<Vec<String>> {
        let options = bollard::query_parameters::LogsOptions {
            tail: tail.to_string(),
            stdout: true,
            stderr: true,
            timestamps: false,
            ..Default::default()
        };

        let stream = self.docker.logs(container_id, Some(options));
        let mut lines = Vec::new();

        let mut stream = Box::pin(stream);
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(LogOutput::StdOut { message }) => {
                    lines.push(String::from_utf8_lossy(&message).to_string());
                }
                Ok(LogOutput::StdErr { message }) => {
                    lines.push(String::from_utf8_lossy(&message).to_string());
                }
                Ok(LogOutput::Console { message }) => {
                    lines.push(String::from_utf8_lossy(&message).to_string());
                }
                Ok(_) => {}
                Err(e) => {
                    warn!("log stream error: {}", e);
                    break;
                }
            }
        }

        Ok(lines)
    }

    pub async fn get_container_logs(
        &self,
        container_id: &str,
        tail: usize,
    ) -> ContainerResult<String> {
        let options = bollard::query_parameters::LogsOptions {
            tail: tail.to_string(),
            stdout: true,
            stderr: true,
            timestamps: false,
            ..Default::default()
        };
        let stream = self.docker.logs(container_id, Some(options));
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut stream = Box::pin(stream);
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(LogOutput::StdOut { message }) => {
                    stdout.push_str(&String::from_utf8_lossy(&message));
                }
                Ok(LogOutput::StdErr { message }) => {
                    stderr.push_str(&String::from_utf8_lossy(&message));
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        Ok(format!("{}\n{}", stdout, stderr))
    }

    pub async fn detect_server_status(&self, container_name: &str) -> super::types::ServerStatus {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::ChangeType;

    fn change(path: &str) -> FilesystemChange {
        FilesystemChange {
            path: path.to_string(),
            kind: ChangeType::_1,
        }
    }

    fn mount(dest: &str) -> VolumeMount {
        VolumeMount::rw("/host/path", dest)
    }

    /// The live scepter sandbox shape: read-only rootfs, tmpfs scratch, and the
    /// bind destinations scepter actually mounts (steps.rs re-adds /home,
    /// /workspace and /workspace-base around the inherited source mounts), plus
    /// the socket dir bind below tmpfs /tmp.
    fn scepter_shape() -> (Vec<FilesystemChange>, Vec<VolumeMount>) {
        let changes = vec![
            // repo mounts inherited from the source (live buffer shape)
            change("/celestia"),
            change("/celestia/entelecheia"),
            change("/celestia/evernight"),
            // workspace mounts scepter re-adds for the fork (steps.rs:664-729)
            change("/workspace"),
            change("/workspace-base"),
            change("/home"),
            // tmpfs scratch + the socket dir bind below it
            change("/tmp"),
            change("/tmp/entelecheia-unix-socket"),
        ];
        let volumes = vec![
            mount("/celestia/entelecheia"),
            mount("/celestia/evernight"),
            mount("/workspace"),
            mount("/workspace-base"),
            mount("/home"),
            mount("/tmp/entelecheia-unix-socket"),
        ];
        (changes, volumes)
    }

    #[test]
    fn fork_fast_path_defaults_to_off() {
        assert!(
            !ForkOptions::default().clean_layer,
            "the clean-layer fast path must stay opt-in until L2 is field-validated"
        );
    }

    #[test]
    fn mountpoint_only_diff_reuses_the_source_image() {
        let (changes, volumes) = scepter_shape();
        assert!(fork_can_reuse_source_image(
            &changes,
            &volumes,
            Some(true),
            Some("sha256:c49ba4b956a5")
        ));
    }

    #[test]
    fn an_empty_diff_reuses_the_source_image() {
        // A brand-new container whose rootfs is read-only reports no changes at all.
        assert!(fork_can_reuse_source_image(
            &[],
            &[mount("/celestia/entelecheia")],
            Some(true),
            Some("sha256:c49ba4b956a5")
        ));
    }

    #[test]
    fn a_deleted_path_forces_a_commit() {
        // A delete means content left the layer, so it is never scaffolding.
        let changes = vec![FilesystemChange {
            path: "/celestia".to_string(),
            kind: ChangeType::_2,
        }];
        let volumes = vec![mount("/celestia/entelecheia")];
        assert!(!fork_can_reuse_source_image(
            &changes,
            &volumes,
            Some(true),
            Some("sha256:c49ba4b956a5")
        ));
    }

    #[test]
    fn real_write_under_home_forces_a_commit() {
        let (mut changes, volumes) = scepter_shape();
        changes.push(change("/home/entelecheia/report.md"));
        assert!(!fork_can_reuse_source_image(
            &changes,
            &volumes,
            Some(true),
            Some("sha256:c49ba4b956a5")
        ));
    }

    #[test]
    fn a_change_below_a_tmpfs_root_is_not_scaffolding() {
        // Seeing /tmp/<x> proves /tmp is a real directory in the layer, so the
        // entry is content and the fork must commit.
        let (mut changes, volumes) = scepter_shape();
        changes.push(change("/tmp/scratch.bin"));
        assert!(!fork_can_reuse_source_image(
            &changes,
            &volumes,
            Some(true),
            Some("sha256:c49ba4b956a5")
        ));
    }

    #[test]
    fn a_change_below_a_bind_destination_is_not_scaffolding() {
        let (mut changes, volumes) = scepter_shape();
        changes.push(change("/celestia/entelecheia/src/lib.rs"));
        assert!(!fork_can_reuse_source_image(
            &changes,
            &volumes,
            Some(true),
            Some("sha256:c49ba4b956a5")
        ));
    }

    #[test]
    fn writable_rootfs_forces_a_commit() {
        let (changes, volumes) = scepter_shape();
        assert!(!fork_can_reuse_source_image(
            &changes,
            &volumes,
            Some(false),
            Some("sha256:c49ba4b956a5")
        ));
        assert!(
            !fork_can_reuse_source_image(&changes, &volumes, None, Some("sha256:c49ba4b956a5")),
            "an unknown rootfs mode is not proof of cleanliness"
        );
    }

    #[test]
    fn unresolvable_source_image_forces_a_commit() {
        let (changes, volumes) = scepter_shape();
        assert!(!fork_can_reuse_source_image(
            &changes,
            &volumes,
            Some(true),
            None
        ));
        assert!(!fork_can_reuse_source_image(
            &changes,
            &volumes,
            Some(true),
            Some("   ")
        ));
    }

    #[test]
    fn nested_bind_destination_marks_its_ancestor_as_scaffolding() {
        let changes = vec![change("/work"), change("/work/sub")];
        let volumes = vec![mount("/work/sub")];
        assert!(fork_can_reuse_source_image(
            &changes,
            &volumes,
            Some(true),
            Some("sha256:aa")
        ));
    }

    #[test]
    fn root_and_empty_paths_are_never_scaffolding() {
        assert!(!is_runtime_scaffold("/", &[]));
        assert!(!is_runtime_scaffold("", &[]));
    }

    #[test]
    fn split_repo_tag_handles_bare_and_tagged_refs() {
        assert_eq!(
            split_repo_tag("cosmos-fork-0190"),
            ("cosmos-fork-0190".to_string(), "latest".to_string())
        );
        assert_eq!(
            split_repo_tag("registry.local/fork:r7"),
            ("registry.local/fork".to_string(), "r7".to_string())
        );
        // A registry port is not a tag: the ':' suffix contains '/'.
        assert_eq!(
            split_repo_tag("ghcr.io:5000/org/img"),
            ("ghcr.io:5000/org/img".to_string(), "latest".to_string())
        );
    }

    /// Integration-shaped exercise of the tag path against a live Docker daemon.
    ///
    /// NOT RUNNABLE in this workspace's verification lane: `sandbox_run.py` runs
    /// with `--network none` and no `/var/run/docker.sock`, so the docker-tests
    /// feature cannot reach a daemon inside the container. Run it on a host with
    /// Docker via `RUN_DOCKER_TESTS=1 cargo test -p cherino --features docker-tests`.
    #[cfg(feature = "docker-tests")]
    #[tokio::test]
    async fn fast_path_tags_the_source_image_instead_of_committing() {
        if std::env::var("RUN_DOCKER_TESTS").as_deref() != Ok("1") {
            eprintln!("skipped: set RUN_DOCKER_TESTS=1 with a live daemon");
            return;
        }
        let manager = match ContainerManager::new() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("skipped: docker unavailable: {e}");
                return;
            }
        };
        let image = std::env::var("CHERINO_TEST_IMAGE").unwrap_or_else(|_| "alpine:3".to_string());
        // Unique names: a rerun after a panicked cleanup must not hit AlreadyExists.
        let run = Uuid::now_v7().to_string();
        let source_name = format!("cherino-u1-source-{run}");
        let source = manager
            .create(&super::super::types::ContainerCreateParams {
                read_only_rootfs: Some(true),
                ..super::super::types::ContainerCreateParams::simple(source_name, image.clone())
            })
            .await
            .expect("create read-only source container");

        let tag = format!("{}{}", COSMOS_FORK_PREFIX, Uuid::now_v7());
        let params = ContainerForkParams {
            source_container_id: source.id.clone(),
            new_name: format!("cherino-u1-fork-{run}"),
            network: None,
            volumes: Vec::new(),
            env: std::collections::HashMap::new(),
            labels: std::collections::HashMap::new(),
            commit_labels: None,
            image_tag: Some(tag.clone()),
            user: None,
            memory_limit: None,
            nano_cpus: None,
            pids_limit: None,
            cap_drop: None,
            cap_add: None,
            security_opt: None,
            read_only_rootfs: Some(true),
        };
        let forked = manager
            .fork_with_options(&params, ForkOptions { clean_layer: true })
            .await
            .expect("fork via fast path");
        // `ContainerInfo::image` is the image *id* (digest), not the tag, so the
        // fork must report the same image the source container came from.
        assert_eq!(
            forked.image, source.image,
            "the fast path must start the fork from the source image"
        );

        // M6 guard: the default entry point must NOT take the fast path. On the
        // same clean read-only source, `fork()` (options default) has to mint a new
        // commit image instead of reusing the source image.
        let plain = ContainerForkParams {
            // Distinct name: `create` rejects a duplicate container name with
            // AlreadyExists, which would make this assertion unreachable.
            new_name: format!("cherino-u1-fork-plain-{run}"),
            image_tag: Some(format!("{}{}", COSMOS_FORK_PREFIX, Uuid::now_v7())),
            ..params.clone()
        };
        let committed = manager.fork(&plain).await.expect("fork via commit path");
        assert_ne!(
            committed.image, source.image,
            "the default fork must commit, not reuse the source image"
        );

        let _ = manager
            .remove_with_image(
                &committed.id,
                plain.image_tag.as_deref().unwrap_or(""),
                true,
            )
            .await;
        let _ = manager.remove_with_image(&forked.id, &tag, true).await;
        let _ = manager
            .remove_with_image(&source.id, "unused-image-name", true)
            .await;
    }
}
