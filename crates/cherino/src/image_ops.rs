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
    types::{ContainerForkParams, ContainerInfo, ForkOptions, ImageInfo},
};

const COSMOS_FORK_PREFIX: &str = "cosmos-fork-";
const COSMOS_SNAPSHOT_PREFIX: &str = "cosmos-snapshot-";

impl SourceForkFacts {
    /// `params.env` over the source's env — the same merge a container gets when it
    /// is created from an image whose Config carries the source env.
    fn merged_env(&self, params: &ContainerForkParams) -> HashMap<String, String> {
        let mut env: HashMap<String, String> = self
            .env
            .iter()
            .filter_map(|entry| entry.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        for (k, v) in &params.env {
            env.insert(k.clone(), v.clone());
        }
        env
    }

    /// `params.labels` over the source's labels.
    fn merged_labels(&self, params: &ContainerForkParams) -> HashMap<String, String> {
        let mut labels = self.labels.clone();
        for (k, v) in &params.labels {
            labels.insert(k.clone(), v.clone());
        }
        labels
    }
}

/// What the fast path needs to know about the source container.
#[derive(Debug, Clone)]
struct SourceForkFacts {
    image_id: String,
    /// The source's own `HostConfig.ReadonlyRootfs`, not the fork's requested one.
    read_only_rootfs: Option<bool>,
    /// Destinations the source really mounts (bind/volume/tmpfs). This — not the
    /// fork's `params.volumes` — is the scaffolding oracle: a diff describes the
    /// source's layer, so only paths the *source* masked are content-free.
    mount_destinations: Vec<String>,
    /// tmpfs roots the source actually reports (`HostConfig.Tmpfs`). Empty means
    /// "no tmpfs", which makes any `/tmp` entry content rather than scaffolding.
    tmpfs_roots: Vec<String>,
    /// `Config.WorkingDir` of the source container. `docker commit` bakes it into
    /// the new image, so the tag path has to carry it into the fork's create
    /// params explicitly or the fork would start in the image's directory.
    working_dir: Option<String>,
    /// `Config.Healthcheck` of the source container — same reasoning: a tag keeps
    /// the *image's* probe, and scepter intentionally overrides it (the cosmos
    /// socket-aware check) at container-create time.
    healthcheck: Option<super::types::HealthcheckParams>,
    /// `Config.Env` of the source container: the fork's env starts from it, with
    /// `params.env` overriding key by key (docker merges container env over image
    /// env, so this reproduces the commit path).
    env: Vec<String>,
    /// `Config.User` of the source container, used when the caller passes none.
    user: Option<String>,
    /// `Config.Labels` of the source container, merged under the caller's labels.
    labels: HashMap<String, String>,
    /// False when the source healthcheck carries a `StartInterval`, which
    /// `HealthcheckParams` cannot express — a tag-path fork would silently change
    /// the probe cadence, so that case fails closed to a commit.
    healthcheck_carryable: bool,
    /// Config fields the fork cannot express through `ContainerCreateParams`
    /// (`Cmd`, `Entrypoint`, `ExposedPorts`, `Volumes`, `StopSignal`). A fork is
    /// only equivalent if these already match the image, because tag-path forks
    /// inherit them from the image instead of from the source container.
    non_representable: NonRepresentableConfig,
}

/// The slice of a container's Config a fork cannot pass to `create`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NonRepresentableConfig {
    cmd: Option<Vec<String>>,
    entrypoint: Option<Vec<String>>,
    exposed_ports: Option<Vec<String>>,
    volumes: Option<Vec<String>>,
    stop_signal: Option<String>,
}

impl NonRepresentableConfig {
    fn build(
        cmd: Option<Vec<String>>,
        entrypoint: Option<Vec<String>>,
        mut exposed_ports: Option<Vec<String>>,
        mut volumes: Option<Vec<String>>,
        stop_signal: Option<String>,
    ) -> Self {
        if let Some(ports) = exposed_ports.as_mut() {
            ports.sort();
        }
        if let Some(v) = volumes.as_mut() {
            v.sort();
        }
        Self {
            cmd,
            entrypoint,
            exposed_ports,
            volumes,
            stop_signal,
        }
    }

    /// From an image's `Config` (what a tag-path fork inherits).
    fn from_image_config(config: Option<&bollard::models::ImageConfig>) -> Self {
        let Some(config) = config else {
            return Self::default();
        };
        Self::build(
            config.cmd.clone(),
            config.entrypoint.clone(),
            config.exposed_ports.clone(),
            config.volumes.clone(),
            config.stop_signal.clone(),
        )
    }

    fn from_config(config: Option<&bollard::models::ContainerConfig>) -> Self {
        let Some(config) = config else {
            return Self::default();
        };
        Self::build(
            config.cmd.clone(),
            config.entrypoint.clone(),
            config.exposed_ports.clone(),
            config.volumes.clone(),
            config.stop_signal.clone(),
        )
    }
}

/// Everything the fast-path decision needs, captured so the decision itself is a
/// pure function that can be unit-tested without a daemon.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FastPathInputs<'a> {
    /// The caller opted in (`ForkOptions::clean_layer`).
    pub(crate) requested: bool,
    /// `docker diff` result; `None` means the daemon returned no body (unknown).
    pub(crate) diff: Option<&'a [FilesystemChange]>,
    pub(crate) source_mounts: &'a [String],
    pub(crate) source_tmpfs: &'a [String],
    pub(crate) read_only_rootfs: Option<bool>,
    pub(crate) image_id: Option<&'a str>,
    /// Source container config equals the image's where a tag cannot carry it.
    pub(crate) configs_match: bool,
    /// The source healthcheck has no `start_interval`, which `HealthcheckParams`
    /// cannot express (so a tag-path fork would silently change the probe cadence).
    pub(crate) healthcheck_carryable: bool,
}

/// The single place that decides whether the clean-layer fast path may run.
///
/// Every gate is conjunctive and every "unknown" answer is a no: an unrequested
/// path, an unreadable diff, a writable or unknown rootfs, an unresolved image, a
/// config the tag cannot carry, or a healthcheck it cannot carry all fall back to
/// the ordinary commit.
pub(crate) fn fast_path_allowed(inputs: &FastPathInputs<'_>) -> bool {
    inputs.requested
        && inputs.configs_match
        && inputs.healthcheck_carryable
        && inputs.read_only_rootfs == Some(true)
        && inputs.image_id.is_some_and(|id| !id.trim().is_empty())
        && inputs.diff.is_some_and(|changes| {
            fork_can_reuse_source_image(
                changes,
                inputs.source_mounts,
                inputs.source_tmpfs,
                inputs.read_only_rootfs,
                inputs.image_id,
            )
        })
}

/// True when a fork may start from the source container's own image instead of
/// committing a new one. All three preconditions must hold:
///
/// 1. the source rootfs is read-only **and** its diff proves the layer is clean
///    (the flag alone is not proof: the sandbox profile grants `SYS_ADMIN`, so a
///    `mount -o remount,rw /` inside the container is possible and would leave
///    the flag set while content reached the layer — the diff catches that);
/// 2. every recorded change is runtime scaffolding — a destination the *source*
///    mounts, an exact tmpfs root the source reports, or an ancestor directory of
///    one of the source's mount destinations (tmpfs ancestors are not honoured);
/// 3. the source image id resolved, so there is an image to fork from.
///
/// Anything else (a real write, a writable or unknown rootfs, an unresolved
/// image) returns `false` and the caller takes the ordinary commit path. The
/// response is a normal `docker tag` of the source image under the fork tag, so
/// [`ContainerManager::remove_with_image`] still finds a `cosmos-fork-*` ref.
pub fn fork_can_reuse_source_image(
    changes: &[FilesystemChange],
    source_mount_destinations: &[String],
    source_tmpfs_roots: &[String],
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
    // No preset fallback on purpose: only tmpfs mounts the SOURCE actually reports
    // can mask layer content. A source created outside cherino (no `--tmpfs`) has a
    // real `/tmp`, where a write shows up as `A /tmp` (a file, and `docker diff`
    // carries no type) — a preset fallback would classify that as scaffolding and
    // silently drop it. With no reported tmpfs, such an entry forces a commit.
    let tmpfs: &[String] = source_tmpfs_roots;
    changes.iter().all(|c| {
        // A delete means real content left the layer, so it is never scaffolding
        // even at a mount path (which a live mount makes undeletable anyway).
        !matches!(c.kind, bollard::models::ChangeType::_2)
            && is_runtime_scaffold(&c.path, source_mount_destinations, tmpfs)
    })
}

/// A diff path that cannot carry container state: an exact tmpfs root of the
/// source, an exact source mount destination, or a strict ancestor directory of
/// a source mount destination.
fn is_runtime_scaffold(path: &str, mount_destinations: &[String], tmpfs_roots: &[String]) -> bool {
    let path = path.trim_end_matches('/');
    if path.is_empty() || path == "/" {
        return false;
    }
    if tmpfs_roots
        .iter()
        .any(|root| root.trim_end_matches('/') == path)
    {
        return true;
    }
    mount_destinations.iter().any(|dest| {
        let dest = dest.trim_end_matches('/');
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

        let mount_destinations: Vec<String> = inspect
            .mounts
            .unwrap_or_default()
            .into_iter()
            .filter_map(|m| m.destination)
            .filter(|d| !d.trim().is_empty())
            .collect();
        let tmpfs_roots: Vec<String> = inspect
            .host_config
            .as_ref()
            .and_then(|h| h.tmpfs.as_ref())
            .map(|tmpfs| tmpfs.keys().cloned().collect())
            .unwrap_or_default();

        let config = inspect.config.as_ref();
        let working_dir = config
            .and_then(|c| c.working_dir.clone())
            .filter(|d| !d.trim().is_empty());
        let healthcheck =
            config
                .and_then(|c| c.healthcheck.as_ref())
                .map(|hc| super::types::HealthcheckParams {
                    test: hc.test.clone().unwrap_or_default(),
                    interval_ns: hc.interval,
                    timeout_ns: hc.timeout,
                    retries: hc.retries,
                    start_period_ns: hc.start_period,
                });

        let env = config.and_then(|c| c.env.clone()).unwrap_or_default();
        let user = config
            .and_then(|c| c.user.clone())
            .filter(|u| !u.trim().is_empty());
        let labels = config.and_then(|c| c.labels.clone()).unwrap_or_default();
        let healthcheck_carryable = config
            .and_then(|c| c.healthcheck.as_ref())
            .and_then(|hc| hc.start_interval)
            .is_none();
        let non_representable = NonRepresentableConfig::from_config(config);

        Ok(SourceForkFacts {
            image_id: inspect.image.clone().unwrap_or_default(),
            read_only_rootfs: inspect.host_config.as_ref().and_then(|h| h.readonly_rootfs),
            mount_destinations,
            tmpfs_roots,
            working_dir,
            healthcheck,
            env,
            user,
            labels,
            healthcheck_carryable,
            non_representable,
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
    ) -> Option<SourceForkFacts> {
        let facts = match self.source_fork_facts(&params.source_container_id).await {
            Ok(facts) => facts,
            Err(e) => {
                warn!(
                    container = %params.source_container_id,
                    error = %e,
                    "fork: source inspect failed, taking the commit path"
                );
                return None;
            }
        };
        // Fail closed: an unreadable diff is "unknown", never "clean".
        // Cheap preconditions first: an opt-in caller with a writable source (or an
        // unresolvable image, or a config a tag cannot carry) must not pay a full
        // recursive layer diff only to commit anyway.
        if facts.read_only_rootfs != Some(true)
            || facts.image_id.trim().is_empty()
            || !facts.healthcheck_carryable
        {
            return None;
        }
        // A tag-path fork inherits Cmd/Entrypoint/ExposedPorts/Volumes/StopSignal from
        // the IMAGE, while a commit-path fork inherits them from the source
        // container. They are safe only when the two already agree.
        let image_config = match self.inspect_image_config(&facts.image_id).await {
            Ok(config) => config,
            Err(e) => {
                warn!(
                    image = %facts.image_id,
                    error = %e,
                    "fork: source image inspect failed, taking the commit path"
                );
                return None;
            }
        };
        if NonRepresentableConfig::from_image_config(image_config.as_ref())
            != facts.non_representable
        {
            warn!(
                container = %params.source_container_id,
                "fork: source container config differs from its image where a tag cannot carry it, taking the commit path"
            );
            return None;
        }

        let changes = match self
            .container_filesystem_changes_checked(&params.source_container_id)
            .await
        {
            Ok(Some(changes)) => changes,
            Ok(None) => {
                warn!(
                    container = %params.source_container_id,
                    "fork: daemon returned no diff (unknown), taking the commit path"
                );
                return None;
            }
            Err(e) => {
                warn!(
                    container = %params.source_container_id,
                    error = %e,
                    "fork: source diff failed, taking the commit path"
                );
                return None;
            }
        };
        if !fast_path_allowed(&FastPathInputs {
            requested: true,
            diff: Some(&changes),
            source_mounts: &facts.mount_destinations,
            source_tmpfs: &facts.tmpfs_roots,
            read_only_rootfs: facts.read_only_rootfs,
            image_id: Some(facts.image_id.as_str()),
            configs_match: true,
            healthcheck_carryable: facts.healthcheck_carryable,
        }) {
            return None;
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
            return None;
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
        Some(facts)
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
        let fast_facts = if options.clean_layer {
            self.reuse_source_image_if_clean(params, &image_tag).await
        } else {
            None
        };
        let reuse_source_image = fast_facts.is_some();

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
            env: fast_facts
                .as_ref()
                .map(|f| f.merged_env(params))
                .unwrap_or_else(|| params.env.clone()),
            ports: Vec::new(),
            volumes: params.volumes.clone(),
            labels: fast_facts
                .as_ref()
                .map(|f| f.merged_labels(params))
                .unwrap_or_else(|| params.labels.clone()),
            // On the fast path the fork image is a tag of the source image, so it
            // does NOT carry the source container's Config. `docker commit` would
            // have baked these in; carry them explicitly instead.
            working_dir: fast_facts.as_ref().and_then(|f| f.working_dir.clone()),
            log_driver: None,
            log_opts: HashMap::new(),
            healthcheck: fast_facts.as_ref().and_then(|f| f.healthcheck.clone()),
            user: params
                .user
                .clone()
                .or_else(|| fast_facts.as_ref().and_then(|f| f.user.clone())),
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

    /// The image's own `Config`, used to prove that the fields a tag cannot carry
    /// already match between the source container and its image.
    async fn inspect_image_config(
        &self,
        image: &str,
    ) -> ContainerResult<Option<bollard::models::ImageConfig>> {
        let inspect = self
            .docker
            .inspect_image(image)
            .await
            .map_err(ContainerError::from)?;
        Ok(inspect.config)
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

    /// Source mount destinations, as `source_fork_facts` derives them from
    /// `inspect.mounts` (the scaffolding oracle).
    fn dests(list: &[&str]) -> Vec<String> {
        list.iter().map(|d| (*d).to_string()).collect()
    }

    /// No tmpfs on the source: `fork_can_reuse_source_image` falls back to the
    /// sandbox preset's scratch roots.
    fn no_tmpfs() -> Vec<String> {
        Vec::new()
    }

    /// The live scepter sandbox shape: read-only rootfs, tmpfs scratch, and the
    /// bind destinations scepter actually mounts (steps.rs re-adds /home,
    /// /workspace and /workspace-base around the inherited source mounts), plus
    /// the socket dir bind below tmpfs /tmp.
    fn scepter_shape() -> (Vec<FilesystemChange>, Vec<String>) {
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
        let dests = dests(&[
            "/celestia/entelecheia",
            "/celestia/evernight",
            "/workspace",
            "/workspace-base",
            "/home",
            "/tmp/entelecheia-unix-socket",
        ]);
        (changes, dests)
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
            &no_tmpfs(),
            Some(true),
            Some("sha256:c49ba4b956a5")
        ));
    }

    #[test]
    fn an_empty_diff_reuses_the_source_image() {
        // A brand-new container whose rootfs is read-only reports no changes at all.
        assert!(fork_can_reuse_source_image(
            &[],
            &dests(&["/celestia/entelecheia"]),
            &no_tmpfs(),
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
        let volumes = dests(&["/celestia/entelecheia"]);
        assert!(!fork_can_reuse_source_image(
            &changes,
            &volumes,
            &no_tmpfs(),
            Some(true),
            Some("sha256:c49ba4b956a5")
        ));
    }

    /// The oracle must be the SOURCE's own mounts, not the fork's requested
    /// `params.volumes`: a destination only the fork will mount never masked
    /// anything in the source's layer, so an entry there is real content.
    /// The ancestor test must respect path segments: a destination `/home` may
    /// never whitelist a diff path `/home2`.
    #[test]
    fn a_sibling_path_prefix_is_not_scaffolding() {
        for (dest, path) in [("/home", "/home2"), ("/workspace", "/workspace-base")] {
            let changes = vec![change(path)];
            assert!(
                !fork_can_reuse_source_image(
                    &changes,
                    &dests(&[dest]),
                    &no_tmpfs(),
                    Some(true),
                    Some("sha256:c49ba4b956a5")
                ),
                "{path} must not be swallowed by a destination {dest}"
            );
        }
    }

    #[test]
    fn a_fork_only_mount_destination_is_not_scaffolding() {
        let changes = vec![change("/data/sub")];
        // The source mounts nothing under /data (it would only be mounted by the
        // fork), so the entry must force a commit.
        assert!(!fork_can_reuse_source_image(
            &changes,
            &[],
            &no_tmpfs(),
            Some(true),
            Some("sha256:c49ba4b956a5")
        ));
        // The same entry IS scaffolding once the source really mounts there.
        assert!(fork_can_reuse_source_image(
            &changes,
            &dests(&["/data/sub"]),
            &no_tmpfs(),
            Some(true),
            Some("sha256:c49ba4b956a5")
        ));
    }

    /// No preset fallback: a source that reports no tmpfs has a real `/tmp`, and a
    /// write there must force a commit rather than be read as a scratch mount.
    #[test]
    fn a_source_tmpfs_entry_without_reported_tmpfs_forces_a_commit() {
        for path in ["/tmp", "/run", "/var/tmp"] {
            let changes = vec![change(path)];
            assert!(
                !fork_can_reuse_source_image(
                    &changes,
                    &[],
                    &[],
                    Some(true),
                    Some("sha256:c49ba4b956a5")
                ),
                "{path} must be content when the source reports no tmpfs"
            );
        }
    }

    #[test]
    fn tmpfs_roots_come_from_the_source_when_reported() {
        let changes = vec![change("/scratch")];
        // A source-declared tmpfs root is scaffolding even though it is not one
        // of the preset's scratch paths.
        assert!(fork_can_reuse_source_image(
            &changes,
            &[],
            &dests(&["/scratch"]),
            Some(true),
            Some("sha256:c49ba4b956a5")
        ));
        // Without a source tmpfs and without a mount, the same path is content.
        assert!(!fork_can_reuse_source_image(
            &changes,
            &[],
            &[],
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
            &no_tmpfs(),
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
            &no_tmpfs(),
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
            &no_tmpfs(),
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
            &no_tmpfs(),
            Some(false),
            Some("sha256:c49ba4b956a5")
        ));
        assert!(
            !fork_can_reuse_source_image(
                &changes,
                &volumes,
                &no_tmpfs(),
                None,
                Some("sha256:c49ba4b956a5")
            ),
            "an unknown rootfs mode is not proof of cleanliness"
        );
    }

    #[test]
    fn unresolvable_source_image_forces_a_commit() {
        let (changes, volumes) = scepter_shape();
        assert!(!fork_can_reuse_source_image(
            &changes,
            &volumes,
            &no_tmpfs(),
            Some(true),
            None
        ));
        assert!(!fork_can_reuse_source_image(
            &changes,
            &volumes,
            &no_tmpfs(),
            Some(true),
            Some("   ")
        ));
    }

    #[test]
    fn nested_bind_destination_marks_its_ancestor_as_scaffolding() {
        let changes = vec![change("/work"), change("/work/sub")];
        let volumes = dests(&["/work/sub"]);
        assert!(fork_can_reuse_source_image(
            &changes,
            &volumes,
            &no_tmpfs(),
            Some(true),
            Some("sha256:aa")
        ));
    }

    #[test]
    fn root_and_empty_paths_are_never_scaffolding() {
        assert!(!is_runtime_scaffold("/", &[], &no_tmpfs()));
        assert!(!is_runtime_scaffold("", &[], &no_tmpfs()));
    }

    /// The tag path inherits Cmd/Entrypoint/ExposedPorts/Volumes/StopSignal from the
    /// image, so a difference there must be detected (and force a commit).
    #[test]
    fn non_representable_config_detects_a_divergence() {
        let build = |cmd: Option<Vec<String>>, volumes: Option<Vec<String>>| {
            NonRepresentableConfig::build(cmd, None, None, volumes, None)
        };
        assert_eq!(
            build(Some(vec!["a".to_string()]), None),
            build(Some(vec!["a".to_string()]), None)
        );
        assert_ne!(
            build(Some(vec!["a".to_string()]), None),
            build(Some(vec!["b".to_string()]), None),
            "a different Cmd must not be considered equivalent"
        );
        assert_ne!(
            build(None, None),
            build(None, Some(vec!["/v".to_string()])),
            "a different volume list must not be considered equivalent"
        );
        // Ordering is normalised, so the same set compares equal.
        assert_eq!(
            build(None, Some(vec!["/b".to_string(), "/a".to_string()])),
            build(None, Some(vec!["/a".to_string(), "/b".to_string()]))
        );
    }

    /// Every gate of the decision, one at a time, without a daemon. The previous
    /// rounds showed these arms were only reachable through the docker-gated test,
    /// so inverting any of them stayed green.
    #[test]
    fn fast_path_decision_pins_all_gates() {
        let clean = [change("/celestia/entelecheia")];
        let mounts = dests(&["/celestia/entelecheia"]);
        let tmpfs = no_tmpfs();
        let base = FastPathInputs {
            requested: true,
            diff: Some(&clean),
            source_mounts: &mounts,
            source_tmpfs: &tmpfs,
            read_only_rootfs: Some(true),
            image_id: Some("sha256:c49ba4b956a5"),
            configs_match: true,
            healthcheck_carryable: true,
        };
        assert!(
            fast_path_allowed(&base),
            "the all-green shape must be allowed"
        );

        let inversions: [(&str, FastPathInputs<'_>); 7] = [
            (
                "not requested",
                FastPathInputs {
                    requested: false,
                    ..base
                },
            ),
            (
                "daemon reported no diff (unknown)",
                FastPathInputs { diff: None, ..base },
            ),
            (
                "writable rootfs",
                FastPathInputs {
                    read_only_rootfs: Some(false),
                    ..base
                },
            ),
            (
                "unknown rootfs",
                FastPathInputs {
                    read_only_rootfs: None,
                    ..base
                },
            ),
            (
                "unresolved image",
                FastPathInputs {
                    image_id: None,
                    ..base
                },
            ),
            (
                "configs differ where a tag cannot carry them",
                FastPathInputs {
                    configs_match: false,
                    ..base
                },
            ),
            (
                "healthcheck not carryable",
                FastPathInputs {
                    healthcheck_carryable: false,
                    ..base
                },
            ),
        ];
        for (label, inputs) in inversions {
            assert!(!fast_path_allowed(&inputs), "must fall back on: {label}");
        }

        // A real write in the diff still forces a commit through the same path.
        let dirty = [change("/home/entelecheia/x")];
        assert!(!fast_path_allowed(&FastPathInputs {
            diff: Some(&dirty),
            ..base
        }));
    }

    /// The carried config must reproduce the commit path: params win, source fills
    /// the gaps, and an env value containing '=' survives intact.
    #[test]
    fn merged_env_and_labels_prefer_the_caller() {
        let facts = SourceForkFacts {
            image_id: "sha256:aa".to_string(),
            read_only_rootfs: Some(true),
            mount_destinations: Vec::new(),
            tmpfs_roots: Vec::new(),
            working_dir: None,
            healthcheck: None,
            env: vec![
                "A=1".to_string(),
                "B=from-source".to_string(),
                "EQ=a=b=c".to_string(),
                "EMPTY=".to_string(),
            ],
            user: Some("1000:1000".to_string()),
            labels: [("k".to_string(), "shared".to_string())]
                .into_iter()
                .collect(),
            healthcheck_carryable: true,
            non_representable: NonRepresentableConfig::default(),
        };
        let mut params = ContainerForkParams {
            source_container_id: "src".to_string(),
            new_name: "fork".to_string(),
            network: None,
            volumes: Vec::new(),
            env: HashMap::new(),
            labels: HashMap::new(),
            commit_labels: None,
            image_tag: None,
            user: None,
            memory_limit: None,
            nano_cpus: None,
            pids_limit: None,
            cap_drop: None,
            cap_add: None,
            security_opt: None,
            read_only_rootfs: None,
        };
        params
            .env
            .insert("B".to_string(), "from-params".to_string());
        params
            .labels
            .insert("k".to_string(), "override".to_string());

        let env = facts.merged_env(&params);
        assert_eq!(env.get("A").map(String::as_str), Some("1"));
        assert_eq!(env.get("B").map(String::as_str), Some("from-params"));
        assert_eq!(env.get("EQ").map(String::as_str), Some("a=b=c"));
        assert_eq!(env.get("EMPTY").map(String::as_str), Some(""));
        let labels = facts.merged_labels(&params);
        assert_eq!(labels.get("k").map(String::as_str), Some("override"));
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
                // Set only on the container (the image has none): the tag path must
                // carry these into the fork's create params, the commit path bakes
                // them into the image, so both forks must end up with them.
                working_dir: Some("/home".to_string()),
                healthcheck: Some(super::super::types::HealthcheckParams {
                    test: vec!["CMD".to_string(), "true".to_string()],
                    interval_ns: Some(30_000_000_000),
                    timeout_ns: Some(5_000_000_000),
                    retries: Some(3),
                    start_period_ns: Some(5_000_000_000),
                }),
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
