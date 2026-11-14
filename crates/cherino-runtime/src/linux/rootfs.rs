use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use nix::unistd::{self, Gid, Uid};
use tracing::{debug, error, info, warn};

use cherino::errors::{ContainerError, ContainerResult};

const ROOTFS_CACHE_DIR: &str = "rootfs";

const HOST_EXCLUDED_DIRS: &[&str] = &["proc", "sys", "dev", "run", "tmp"];

/// Environment opt-in for the host-rootfs (`lowerdir=/`) overlay mode.
///
/// Mounting the container rootfs as an overlay over the host `/` would let the
/// container read the entire host filesystem (a read escape), so this mode is
/// fail-closed by default. It only runs when this flag is explicitly set — a
/// local-development escape hatch, never for production.
const HOST_ROOTFS_ALLOW_ENV: &str = "ENTELECHEIA_ALLOW_HOST_ROOTFS";

/// Return true when the host-rootfs overlay escape hatch is explicitly enabled.
///
/// Defaults to `false` (fail-closed) so a container can never silently read the
/// host filesystem via `lowerdir=/`.
fn host_rootfs_allowed() -> bool {
    std::env::var(HOST_ROOTFS_ALLOW_ENV)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
pub struct RootfsManager {
    base_dir: PathBuf,
}

impl RootfsManager {
    pub fn new(base_dir: &Path) -> Self {
        Self {
            base_dir: base_dir.join(ROOTFS_CACHE_DIR),
        }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.base_dir
    }

    /// SECURITY (perfsec sweep 2026-11-14, fixed 2026-11-15): the charset
    /// replace below leaves "." and ".." untouched, and
    /// `PathBuf::join("..")` escapes the cache root — `remove_image` would
    /// then recursively delete the DATA root. Names failing the same core
    /// rules as [`Self::validate_path_component`] map to one inert constant
    /// directory instead (stays total, cannot escape); the reachable
    /// callers all feed system-generated refs, this is defense in depth.
    pub fn base_rootfs_path(&self, image: &str) -> PathBuf {
        let safe_name = if image.is_empty()
            || image == "."
            || image == ".."
            || image.contains("..")
            || image.contains('/')
            || image.contains('\\')
            || image.contains('\0')
        {
            // Injective per-input sink (review S2, 2026-11-15): one shared
            // constant made every rejected name collide — a first rejected
            // pull would short-circuit `is_dir()` for all the others and a
            // interrupted unpack would block them forever. Hex-encoding the
            // raw name gives each evil input its own directory and can never
            // equal a legit `replace` output of a *different* image (the
            // only fixed point would be an image literally named like
            // another image's full hex encoding — absurd, and still
            // cache-level even then).
            format!(
                "_rejected_{}",
                image
                    .bytes()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            )
        } else {
            image.replace(['/', ':', '@'], "_")
        };
        self.base_dir.join(safe_name)
    }

    /// Audit cherino-R1-1: validate the container id before joining —
    /// `Path::join` does not normalize, so a caller-controlled name
    /// containing `..` or absolute-path components could escape the
    /// containers directory entirely.
    pub fn container_rootfs_path(&self, container_id: &str) -> ContainerResult<PathBuf> {
        Self::validate_path_component(container_id, "container_id")?;
        Ok(self.base_dir.join("containers").join(container_id))
    }

    /// Audit cherino-R1-1: resolve `candidate` under `base` and reject
    /// anything that escapes it after canonicalization (download /
    /// upload archive paths).
    pub fn resolve_under(
        base: &std::path::Path,
        candidate: &std::path::Path,
    ) -> ContainerResult<std::path::PathBuf> {
        let joined = base.join(candidate);
        let canonical_base = base
            .canonicalize()
            .map_err(|e| ContainerError::OperationFailed {
                container_id: String::new(),
                message: format!("canonicalize base {:?}: {}", base, e),
            })?;
        // The leaf may not exist yet (upload creates it), so canonicalize
        // the deepest existing ancestor and re-join the remainder.
        let mut canonical = joined.clone();
        let mut stripped = Vec::new();
        while !canonical.exists() {
            match (canonical.parent(), canonical.file_name()) {
                (Some(p), Some(f)) => {
                    stripped.push(std::path::PathBuf::from(f));
                    canonical = p.to_path_buf();
                }
                _ => break,
            }
        }
        let canonical = canonical
            .canonicalize()
            .map_err(|e| ContainerError::OperationFailed {
                container_id: String::new(),
                message: format!("canonicalize {:?}: {}", canonical, e),
            })?;
        let mut full = canonical;
        for part in stripped.iter().rev() {
            full = full.join(part);
        }
        if !full.starts_with(&canonical_base) {
            return Err(ContainerError::OperationFailed {
                container_id: String::new(),
                message: format!(
                    "path {:?} escapes the container rootfs {:?}",
                    candidate, canonical_base
                ),
            });
        }
        Ok(full)
    }

    /// Reject empty names, `.`/`..` segments, embedded separators, and
    /// anything outside the safe identifier charset.
    pub fn validate_path_component(value: &str, label: &str) -> ContainerResult<()> {
        if value.is_empty() {
            return Err(ContainerError::OperationFailed {
                container_id: String::new(),
                message: format!("{} must not be empty", label),
            });
        }
        if value == "." || value == ".." || value.contains("..") {
            return Err(ContainerError::OperationFailed {
                container_id: String::new(),
                message: format!("{} must not contain '..' segments: {:?}", label, value),
            });
        }
        if value.contains('/') || value.contains('\\') || value.contains('\0') {
            return Err(ContainerError::OperationFailed {
                container_id: String::new(),
                message: format!("{} must not contain path separators", label),
            });
        }
        let first = value.chars().next().unwrap();
        let rest_ok = value
            .chars()
            .skip(1)
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
        if !(first.is_ascii_alphanumeric() && rest_ok) {
            return Err(ContainerError::OperationFailed {
                container_id: String::new(),
                message: format!(
                    "{} must match [a-zA-Z0-9][a-zA-Z0-9_.-]*: {:?}",
                    label, value
                ),
            });
        }
        Ok(())
    }

    /// Audit cherino-R1-1: same traversal validation as container ids.
    pub fn snapshot_path(&self, snapshot_id: &str) -> ContainerResult<PathBuf> {
        Self::validate_path_component(snapshot_id, "snapshot_id")?;
        Ok(self.base_dir.join("snapshots").join(snapshot_id))
    }

    pub async fn ensure_cache_dir(&self) -> ContainerResult<()> {
        tokio::fs::create_dir_all(&self.base_dir)
            .await
            .map_err(|e| ContainerError::OperationFailed {
                container_id: "rootfs".to_string(),
                message: format!("failed to create rootfs cache dir: {}", e),
            })?;
        tokio::fs::create_dir_all(self.base_dir.join("containers"))
            .await
            .map_err(|e| ContainerError::OperationFailed {
                container_id: "rootfs".to_string(),
                message: format!("failed to create containers dir: {}", e),
            })?;
        tokio::fs::create_dir_all(self.base_dir.join("snapshots"))
            .await
            .map_err(|e| ContainerError::OperationFailed {
                container_id: "rootfs".to_string(),
                message: format!("failed to create snapshots dir: {}", e),
            })?;
        self.normalize_ownership(&self.base_dir).await?;
        Ok(())
    }

    /// Infer the `(uid, gid)` that rootfs artifacts *should* belong to.
    ///
    /// When the runtime runs with elevated privileges — most notably the
    /// `e-scepter` container, which executes as root while the host user's
    /// `~/.config/entelecheia` is bind-mounted in — every file we create is
    /// implicitly root-owned and becomes impossible for the real user to clean
    /// up afterwards (the very situation that left root-owned `proc`/`dev`/
    /// `sys` residue stuck in the config directory).
    ///
    /// We resolve the intended owner by walking up from `base_dir` and taking
    /// the first ancestor owned by a non-root account — normally the user's
    /// config directory. Returns `None` when no non-root owner can be inferred
    /// (i.e. we are legitimately running for a root user).
    fn intended_owner(&self) -> Option<(u32, u32)> {
        let mut current = self.base_dir.clone();
        loop {
            if let Ok(meta) = std::fs::metadata(&current) {
                let (uid, gid) = (meta.uid(), meta.gid());
                if uid != 0 {
                    return Some((uid, gid));
                }
            }
            if !current.pop() {
                return None;
            }
        }
    }

    /// Recursively hand ownership of `path` back to the real user whenever the
    /// process is running as root but a non-root owner can be inferred.
    ///
    /// This is a no-op for the common rootless path (the intended design) and
    /// only compensates for the privileged fallback so that no root-owned
    /// artifact ever leaks into the user's config directory — guaranteeing that
    /// cleanup never requires root.
    async fn normalize_ownership(&self, path: &Path) -> ContainerResult<()> {
        if !unistd::geteuid().is_root() {
            return Ok(());
        }
        let Some((uid, gid)) = self.intended_owner() else {
            return Ok(());
        };
        if uid == 0 {
            return Ok(());
        }

        let target = path.to_path_buf();
        let cid_tag = "rootfs".to_string();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            fn chown_tree(path: &Path, uid: u32, gid: u32) -> std::io::Result<()> {
                unistd::chown(path, Some(Uid::from_raw(uid)), Some(Gid::from_raw(gid))).map_err(
                    |e| std::io::Error::other(format!("chown {}: {}", path.display(), e)),
                )?;
                let meta = std::fs::metadata(path)?;
                if meta.is_dir() {
                    for entry in std::fs::read_dir(path)? {
                        let entry = entry?;
                        chown_tree(&entry.path(), uid, gid)?;
                    }
                }
                Ok(())
            }
            chown_tree(&target, uid, gid)
        })
        .await
        .map_err(|e| ContainerError::OperationFailed {
            container_id: cid_tag.clone(),
            message: format!("ownership normalization join failed: {}", e),
        })?
        .map_err(|e| ContainerError::OperationFailed {
            container_id: cid_tag,
            message: format!("failed to normalize ownership of {}: {}", path.display(), e),
        })?;
        debug!(
            path = %path.display(),
            uid, gid,
            "normalized rootfs ownership back to invoking user (was running as root)"
        );
        Ok(())
    }

    pub fn is_host_rootfs_image(image: &str) -> bool {
        matches!(image, "host" | "entelecheia" | "cosmos" | "cosmos-host")
            || image.starts_with("host:")
    }

    pub async fn prepare_container_rootfs(
        &self,
        base_image: &str,
        container_id: &str,
    ) -> ContainerResult<PathBuf> {
        if Self::is_host_rootfs_image(base_image) {
            self.prepare_overlay_rootfs(container_id).await
        } else {
            let base_path = self.base_rootfs_path(base_image);
            if base_path.is_dir() {
                let container_path = self.container_rootfs_path(container_id)?;
                self.normalize_ownership(&container_path).await?;
                Ok(container_path)
            } else {
                self.prepare_overlay_rootfs(container_id).await
            }
        }
    }

    async fn prepare_overlay_rootfs(&self, container_id: &str) -> ContainerResult<PathBuf> {
        // Fail-closed: an overlay with `lowerdir=/` exposes the entire host
        // filesystem to the container (a read escape). Refuse to build such a
        // rootfs unless the operator explicitly opts in via the escape-hatch
        // flag (local development only).
        if !host_rootfs_allowed() {
            return Err(ContainerError::OperationFailed {
                container_id: container_id.to_string(),
                message: format!(
                    "host rootfs overlay (lowerdir=/) is disabled: it would expose the entire \
                     host filesystem to the container. Set {}=1 to opt in for local \
                     development only.",
                    HOST_ROOTFS_ALLOW_ENV
                ),
            });
        }

        let container_path = self.container_rootfs_path(container_id)?;
        let upperdir = container_path.join("upper");
        let workdir = container_path.join("work");
        let merged = container_path.join("merged");

        if merged.exists() {
            return Ok(merged);
        }

        tokio::fs::create_dir_all(&upperdir).await.map_err(|e| {
            ContainerError::OperationFailed {
                container_id: container_id.to_string(),
                message: format!("mkdir upper: {}", e),
            }
        })?;
        tokio::fs::create_dir_all(&workdir)
            .await
            .map_err(|e| ContainerError::OperationFailed {
                container_id: container_id.to_string(),
                message: format!("mkdir work: {}", e),
            })?;
        tokio::fs::create_dir_all(&merged)
            .await
            .map_err(|e| ContainerError::OperationFailed {
                container_id: container_id.to_string(),
                message: format!("mkdir merged: {}", e),
            })?;

        // Hand the freshly created upper/work/merged dirs back to the real user
        // before mounting so the overlay artifacts are never root-owned.
        self.normalize_ownership(&container_path).await?;

        let options = format!(
            "lowerdir=/,upperdir={},workdir={}",
            upperdir.display(),
            workdir.display()
        );

        let merged_str = merged.to_string_lossy().to_string();

        let fuse_bin =
            std::env::var("FUSE_OVERLAYFS_BIN").unwrap_or_else(|_| "fuse-overlayfs".to_string());

        let fuse_available = tokio::process::Command::new(&fuse_bin)
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

        if fuse_available {
            let status = tokio::process::Command::new(&fuse_bin)
                .args([
                    "-o",
                    &format!(
                        "lowerdir=/,upperdir={},workdir={}",
                        upperdir.display(),
                        workdir.display()
                    ),
                    &merged_str,
                ])
                .status()
                .await;

            if status.map(|s| s.success()).unwrap_or(false) {
                Self::clear_host_excluded_dirs(&merged, container_id).await?;
                info!(
                    container_id = %container_id,
                    merged = %merged_str,
                    "host rootfs overlay mounted via fuse-overlayfs (user-space, no CAP_SYS_ADMIN needed)"
                );
                return Ok(merged);
            }

            let err_output = tokio::process::Command::new(&fuse_bin)
                .args(["-o", &options, &merged_str])
                .output()
                .await;
            let stderr = err_output
                .map(|o| String::from_utf8_lossy(&o.stderr).to_string())
                .unwrap_or_default();
            error!(
                container_id,
                stderr = %stderr.trim(),
                "fuse-overlayfs mount failed"
            );
        }

        let output = tokio::process::Command::new("mount")
            .args(["-t", "overlay", "overlay", "-o", &options, &merged_str])
            .output()
            .await
            .map_err(|e| ContainerError::OperationFailed {
                container_id: container_id.to_string(),
                message: format!("failed to execute mount: {}", e),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = tokio::fs::remove_dir_all(&container_path).await;
            error!(
                container_id,
                exit_code = %output.status,
                stderr = %stderr.trim(),
                "overlayfs mount FAILED — rootfs isolation unavailable"
            );
            return Err(ContainerError::OperationFailed {
                container_id: container_id.to_string(),
                message: format!(
                    "overlayfs mount failed (exit {}). stderr: {}. \
                     Install fuse-overlayfs or grant CAP_SYS_ADMIN. \
                     Rootfs isolation is MANDATORY for WRITE mode.",
                    output.status,
                    stderr.trim()
                ),
            });
        }

        Self::clear_host_excluded_dirs(&merged, container_id).await?;

        info!(
            container_id = %container_id,
            merged = %merged_str,
            "host rootfs overlay mounted (sandbox mode)"
        );

        Ok(merged)
    }

    /// Audit cherino-R1-3: clear the host-excluded directories under the
    /// mounted overlay. A failed removal leaves the lowerdir's contents
    /// visible inside the container (in the lowerdir=/ development mode
    /// that means host /proc|/sys|/dev) — the mount is declared FAILED
    /// instead of silently proceeding with a weakened isolation.
    async fn clear_host_excluded_dirs(
        merged: &std::path::Path,
        container_id: &str,
    ) -> ContainerResult<()> {
        for excluded in HOST_EXCLUDED_DIRS {
            let dir = merged.join(excluded);
            if dir.is_dir() {
                if let Err(e) = tokio::fs::remove_dir_all(&dir).await {
                    return Err(ContainerError::OperationFailed {
                        container_id: container_id.to_string(),
                        message: format!(
                            "cannot clear host-excluded dir {excluded:?} in the overlay: {e} — \
                             isolation would be weakened, refusing to mount"
                        ),
                    });
                }
                if let Err(e) = tokio::fs::create_dir(&dir).await {
                    return Err(ContainerError::OperationFailed {
                        container_id: container_id.to_string(),
                        message: format!(
                            "cannot recreate mount point {excluded:?} in the overlay: {e}"
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    pub async fn cleanup_container_rootfs(&self, container_id: &str) -> ContainerResult<()> {
        let container_path = self.container_rootfs_path(container_id)?;
        let merged = container_path.join("merged");

        if merged.is_dir() {
            let merged_str = merged.to_string_lossy().to_string();
            Self::force_unmount(&merged_str).await;
        }

        if container_path.exists() {
            let _ = self.normalize_ownership(&container_path).await;
            match tokio::fs::remove_dir_all(&container_path).await {
                Ok(()) => {}
                Err(e) => {
                    warn!(
                        path = %container_path.display(),
                        error = %e,
                        "remove_dir_all failed after unmount, retrying with lazy unmount"
                    );
                    let merged_str = container_path.join("merged").to_string_lossy().to_string();
                    Self::force_unmount(&merged_str).await;
                    let _ = tokio::fs::remove_dir_all(&container_path).await;
                }
            }
        }
        Ok(())
    }

    /// Scan the `containers/` directory and force-clean every stale FUSE
    /// mount left behind by a crash, restart, or failed cleanup.
    ///
    /// Called during startup (`YoukiManager::initialize`) **before**
    /// `reconcile` so that stale mounts never accumulate across server
    /// restarts.
    pub async fn cleanup_stale_mounts(&self) -> usize {
        let containers_dir = self.base_dir.join("containers");
        let mut cleaned = 0usize;

        let mut entries = match tokio::fs::read_dir(&containers_dir).await {
            Ok(rd) => rd,
            Err(_) => return 0,
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let container_path = entry.path();
            let merged = container_path.join("merged");

            if merged.is_dir() {
                let merged_str = merged.to_string_lossy().to_string();
                Self::force_unmount(&merged_str).await;
                cleaned += 1;
            }

            let _ = self.normalize_ownership(&container_path).await;
            let _ = tokio::fs::remove_dir_all(&container_path).await;
        }

        if cleaned > 0 {
            info!(
                cleaned,
                "startup sweep: cleaned {} stale container rootfs mount(s)", cleaned
            );
        }

        cleaned
    }

    /// Escalating unmount strategy for FUSE / overlay mounts.
    ///
    /// `fusermount -u` is the *only* method that reliably terminates the
    /// userspace `fuse-overlayfs` daemon. Plain `umount` detaches the VFS
    /// entry but often leaves the daemon alive, which is the root cause of
    /// the fuse-overlayfs process leak. We escalate through:
    ///
    /// 1. `fusermount3 -u`   — modern FUSE unmount (kills daemon)
    /// 2. `fusermount  -u`   — older FUSE unmount  (kills daemon)
    /// 3. `umount -l`        — lazy kernel unmount (VFS detach, daemon dies
    ///    when last file descriptor closes)
    /// 4. `umount`           — last-resort synchronous unmount
    async fn force_unmount(path: &str) {
        for (cmd, args) in [
            ("fusermount3", vec!["-u", path]),
            ("fusermount", vec!["-u", path]),
            ("umount", vec!["-l", path]),
            ("umount", vec![path]),
        ] {
            match tokio::process::Command::new(cmd).args(&args).status().await {
                Ok(s) if s.success() => {
                    debug!(path, unmounter = cmd, "unmount succeeded");
                    return;
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        warn!(
            path,
            "all unmount methods failed — fuse-overlayfs daemon may linger"
        );
    }

    pub async fn snapshot_rootfs(
        &self,
        container_id: &str,
        snapshot_id: &str,
    ) -> ContainerResult<PathBuf> {
        let src = self.container_rootfs_path(container_id)?;
        let dest = self.snapshot_path(snapshot_id)?;

        if !src.exists() {
            return Err(ContainerError::NotFound(format!(
                "container rootfs not found: {}",
                src.display()
            )));
        }

        let upperdir = src.join("upper");
        if upperdir.is_dir() && Self::dir_is_nonempty(&upperdir).await {
            self.copy_rootfs(&upperdir, &dest).await?;
        } else {
            tokio::fs::create_dir_all(&dest).await.map_err(|e| {
                ContainerError::OperationFailed {
                    container_id: container_id.to_string(),
                    message: format!("mkdir snapshot: {}", e),
                }
            })?;
        }

        Ok(dest)
    }

    pub async fn extract_rootfs(&self, tarball: &Path, dest: &Path) -> ContainerResult<()> {
        if dest.exists() {
            return Ok(());
        }

        // SECURITY (tar-validate): reject hostile members BEFORE any
        // filesystem effect — an earlier position left an empty dest on
        // refusal, which pull_image's `dest.is_dir()` short-circuit then
        // mistook for a cached image (fake success, empty rootfs).
        Self::validate_tarball(tarball)?;

        tokio::fs::create_dir_all(dest)
            .await
            .map_err(|e| ContainerError::OperationFailed {
                container_id: "rootfs".to_string(),
                message: format!("mkdir {}: {}", dest.display(), e),
            })?;
        let dest_str = dest.to_string_lossy().to_string();
        let tarball_str = tarball.to_string_lossy().to_string();

        let status = tokio::process::Command::new("tar")
            .args([
                "-xzf",
                &tarball_str,
                "-C",
                &dest_str,
                "--numeric-owner",
                "--preserve-permissions",
            ])
            .status()
            .await
            .map_err(|e| ContainerError::OperationFailed {
                container_id: "rootfs".to_string(),
                message: format!("tar: {}", e),
            })?;

        if !status.success() {
            return Err(ContainerError::OperationFailed {
                container_id: "rootfs".to_string(),
                message: format!("tar failed: {}", status),
            });
        }

        Ok(())
    }

    /// SECURITY (tar-validate, 2026-11-15): read-only pre-scan of a rootfs
    /// tarball BEFORE GNU tar extracts it. The archive arrives over the
    /// network (`pull_image`), so "GNU tar refuses bad members on extract"
    /// was the only line of defense — this makes the check explicit,
    /// deterministic and testable offline. Rejects:
    ///   * absolute member names (leading `/`);
    ///   * any `..` path component;
    ///   * symlink/hardlink members whose target escapes the extraction
    ///     root (absolute target, or a `..`-containing relative one);
    ///   * any member whose path traverses an earlier symlink member
    ///     that points outside (the classic write-through escape).
    ///
    /// Pure decision core over `(kind, name, link_target)` triples so unit
    /// tests need no archives; [`Self::validate_tarball`] feeds it real ones.
    pub fn validate_members(members: &[(char, String, Option<String>)]) -> ContainerResult<()> {
        // Resolve `comps` starting from `base` through the symlink table.
        // Absolute targets restart at the archive root. Every hop that
        // would pop above the root is an escape; chained links are
        // followed with a depth cap (a cycle can only come from a hostile
        // archive — real images never loop).
        fn resolve(
            base: &[String],
            comps: &[&str],
            links: &std::collections::HashMap<Vec<String>, (Vec<String>, bool)>,
            depth: &mut u32,
        ) -> Result<Vec<String>, String> {
            *depth += 1;
            if *depth > 64 {
                return Err("symlink chain too deep (cycle?)".into());
            }
            let mut cur: Vec<String> = base.to_vec();
            for comp in comps {
                match *comp {
                    "" | "." => {}
                    ".." => {
                        if cur.pop().is_none() {
                            return Err("component '..' escapes the archive root".to_string());
                        }
                    }
                    c => {
                        cur.push(c.to_string());
                        if let Some((target, absolute)) = links.get(&cur) {
                            if *absolute {
                                // The KERNEL resolves an absolute symlink
                                // target against the HOST root — writing a
                                // member through it lands outside dest no
                                // matter what the chroot semantics are at
                                // runtime. (An absolute link nobody
                                // traverses is the legitimate rootfs shape
                                // and stays accepted.)
                                return Err(
                                    "member traverses symlink with an absolute target".to_string()
                                );
                            }
                            let t: Vec<&str> = target.iter().map(|x| x.as_str()).collect();
                            let resolved = resolve(&cur[..cur.len() - 1], &t, links, depth)?;
                            cur = resolved;
                        }
                    }
                }
            }
            Ok(cur)
        }

        let err = |msg: String| ContainerError::OperationFailed {
            container_id: "rootfs".into(),
            message: msg,
        };

        // Symlink table: CANONICAL path -> raw target components. Canonical
        // because a symlink may itself live behind another symlink (`/bin`
        // behind `/bin -> usr/bin`); we register the resolved location.
        //
        // KNOWN LIMITATION (review S3, non-exploitable): the table only
        // knows links that appeared EARLIER in the archive. A child link
        // registered before its prefix link (`b/c -> X` then `b -> d`)
        // keeps its pre-prefix canonical spot, so a member written through
        // the later shape can slip past the resolver. GNU tar itself
        // refuses that extraction ("Cannot create symlink ... File
        // exists", exit 2) — verified with a crafted archive — so the
        // write never lands; the resolver is the second line, not the
        // only one.
        let mut links: std::collections::HashMap<Vec<String>, (Vec<String>, bool)> =
            std::collections::HashMap::new();

        for (kind, name, target) in members {
            let path = name.trim_start_matches("./");
            if path.starts_with('/') {
                return Err(err(format!("tar member {name:?} is absolute")));
            }
            if path.split('/').any(|c| c == "..") {
                return Err(err(format!("tar member {name:?} contains '..'")));
            }
            let comps: Vec<&str> = path
                .split('/')
                .filter(|c| !c.is_empty() && *c != ".")
                .collect();

            match kind {
                'l' | 'h' => {
                    let Some(t) = target else {
                        return Err(err(format!("link member {name:?} has no target")));
                    };
                    // Resolve the link's OWN location first (it may sit
                    // behind an earlier symlink).
                    let at = resolve(&[], &comps, &links, &mut 0).map_err(err)?;
                    if *kind == 'h' {
                        // A hardlink target names another MEMBER: reject any
                        // escaping shape outright (real members never need
                        // absolute or '..' paths here).
                        let tcomps: Vec<&str> = t.split('/').filter(|c| !c.is_empty()).collect();
                        if t.starts_with('/') || tcomps.contains(&"..") {
                            return Err(err(format!(
                                "hardlink {name:?} -> {t:?} names a path outside the archive"
                            )));
                        }
                        let _ = resolve(&[], &tcomps, &links, &mut 0).map_err(err)?;
                    } else {
                        // A symlink target may legitimately be absolute
                        // (rootfs images are FULL of `/bin -> usr/bin`
                        // shapes — they resolve inside the runtime chroot)
                        // or contain '..' while staying net-inside
                        // (`/var/run -> ../run`). The extraction-time danger
                        // is only members written THROUGH such a link; the
                        // resolver below decides that per member. Register
                        // the raw target at the link's canonical location.
                        let absolute = t.starts_with('/');
                        let raw: Vec<String> = t
                            .split('/')
                            .filter(|c| !c.is_empty())
                            .map(String::from)
                            .collect();
                        links.insert(at, (raw, absolute));
                    }
                }
                _ => {
                    // Any member extracted THROUGH a symlink resolves here:
                    // if following the chain ever leaves the archive root,
                    // GNU tar would write outside dest.
                    resolve(&[], &comps, &links, &mut 0).map_err(err)?;
                }
            }
        }
        Ok(())
    }

    /// Feed a real `.tar.gz` through the pure core: iterate members WITHOUT
    /// unpacking (tar's `entries()` never writes), map entry types to the
    /// one-char kinds, then delegate to [`Self::validate_members`].
    pub fn validate_tarball(path: &Path) -> ContainerResult<()> {
        let file = std::fs::File::open(path).map_err(|e| ContainerError::OperationFailed {
            container_id: "rootfs".into(),
            message: format!("open {}: {}", path.display(), e),
        })?;
        let gz = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(gz);
        let mut members: Vec<(char, String, Option<String>)> = Vec::new();
        for entry in archive
            .entries()
            .map_err(|e| ContainerError::OperationFailed {
                container_id: "rootfs".into(),
                message: format!("tar iterate: {e}"),
            })?
        {
            let entry = entry.map_err(|e| ContainerError::OperationFailed {
                container_id: "rootfs".into(),
                message: format!("tar entry: {e}"),
            })?;
            let header = entry.header();
            let kind = match header.entry_type() {
                tar::EntryType::Regular | tar::EntryType::Continuous => 'f',
                tar::EntryType::Symlink => 'l',
                tar::EntryType::Link => 'h',
                tar::EntryType::Directory => 'd',
                _ => 'o',
            };
            let name = header
                .path()
                .map_err(|e| ContainerError::OperationFailed {
                    container_id: "rootfs".into(),
                    message: format!("tar path: {e}"),
                })?
                .to_string_lossy()
                .to_string();
            let target = match header.link_name() {
                Ok(Some(t)) => Some(t.to_string_lossy().to_string()),
                _ => None,
            };
            members.push((kind, name, target));
        }
        if members.is_empty() {
            return Err(ContainerError::OperationFailed {
                container_id: "rootfs".into(),
                message: "tarball has no members".into(),
            });
        }
        Self::validate_members(&members)
    }

    pub async fn copy_rootfs(&self, src: &Path, dest: &Path) -> ContainerResult<()> {
        if dest.exists() {
            return Ok(());
        }

        let src_str = src.to_string_lossy().to_string();
        let dest_str = dest.to_string_lossy().to_string();

        let status = tokio::process::Command::new("cp")
            .args(["-a", "--reflink=auto", &src_str, &dest_str])
            .status()
            .await
            .map_err(|e| ContainerError::OperationFailed {
                container_id: "rootfs".to_string(),
                message: format!("cp: {}", e),
            })?;

        if !status.success() {
            return Err(ContainerError::OperationFailed {
                container_id: "rootfs".to_string(),
                message: format!("cp failed: {}", status),
            });
        }

        Ok(())
    }

    async fn dir_is_nonempty(path: &Path) -> bool {
        if let Ok(mut entries) = tokio::fs::read_dir(path).await {
            entries
                .next_entry()
                .await
                .map(|e| e.is_some())
                .unwrap_or(false)
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Restore `HOST_ROOTFS_ALLOW_ENV` to its prior value on drop, mirroring
    /// the env-guard pattern used elsewhere in this crate.
    struct EnvGuard {
        prev: Option<String>,
    }

    impl EnvGuard {
        fn clear() -> Self {
            let prev = std::env::var(HOST_ROOTFS_ALLOW_ENV).ok();
            unsafe { std::env::remove_var(HOST_ROOTFS_ALLOW_ENV) };
            Self { prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => unsafe { std::env::set_var(HOST_ROOTFS_ALLOW_ENV, v) },
                None => unsafe { std::env::remove_var(HOST_ROOTFS_ALLOW_ENV) },
            }
        }
    }

    /// Both checks mutate the same process-wide environment variable, so they
    /// run sequentially inside one test to avoid racing each other under the
    /// default parallel test harness.
    #[tokio::test]
    async fn host_rootfs_flag_and_overlay_fail_closed_by_default() {
        {
            let _guard = EnvGuard::clear();
            assert!(!host_rootfs_allowed(), "default must be fail-closed");
            unsafe { std::env::set_var(HOST_ROOTFS_ALLOW_ENV, "1") };
            assert!(host_rootfs_allowed(), "=1 must enable the escape hatch");
            unsafe { std::env::set_var(HOST_ROOTFS_ALLOW_ENV, "true") };
            assert!(host_rootfs_allowed(), "=true must enable the escape hatch");
            unsafe { std::env::set_var(HOST_ROOTFS_ALLOW_ENV, "0") };
            assert!(!host_rootfs_allowed(), "=0 must stay disabled");
            unsafe { std::env::set_var(HOST_ROOTFS_ALLOW_ENV, "no") };
            assert!(!host_rootfs_allowed(), "garbage must stay disabled");
        }

        let _guard = EnvGuard::clear();
        let tmp = tempfile::tempdir().expect("tempdir");
        let mgr = RootfsManager::new(tmp.path());

        let result = mgr
            .prepare_container_rootfs("host", "fail-closed-test")
            .await;

        let err = result.expect_err("host rootfs overlay must fail closed by default");
        let msg = format!("{err}");
        assert!(
            msg.contains(HOST_ROOTFS_ALLOW_ENV),
            "error must document the opt-in flag, got: {msg}"
        );
        assert!(
            msg.contains("host filesystem"),
            "error must explain the read-escape risk, got: {msg}"
        );
    }

    #[tokio::test]
    async fn non_host_image_without_cache_also_fails_closed() {
        // The fallback path (image not pulled -> overlay) also uses the host `/`
        // lowerdir and must fail closed rather than silently exposing the host.
        let _guard = EnvGuard::clear();
        let tmp = tempfile::tempdir().expect("tempdir");
        let mgr = RootfsManager::new(tmp.path());

        let result = mgr
            .prepare_container_rootfs("some-image:latest", "fallback-test")
            .await;

        assert!(
            result.is_err(),
            "unpulled-image overlay fallback must fail closed by default"
        );
    }
}

#[cfg(test)]
mod path_validation_tests {
    use super::*;

    // SECURITY (tar-validate, 2026-11-15): the pure decision core.
    // Semantics after the review-driven rewrite: absolute and `..`
    // SYMLINK TARGETS ARE LEGITIMATE (real distro images are full of
    // them — Alpine is 307/339 absolute); what must be rejected is any
    // member whose extraction would WRITE outside the root (through a
    // resolving-outside symlink chain), plus absolute/`..` member names
    // and escaping hardlink targets. Cases run to completion (no
    // short-circuit) so every shape is actually exercised.
    #[test]
    fn validate_members_rejects_hostile_shapes() {
        type M = (char, String, Option<String>);
        let bad: Vec<(&str, Vec<M>)> = vec![
            (
                "absolute member name",
                vec![('f', "/etc/passwd".into(), None)],
            ),
            (
                "dotdot in member name",
                vec![('f', "./../../etc".into(), None)],
            ),
            ("dotdot mid-path", vec![('f', "a/../../b".into(), None)]),
            ("bare dotdot member", vec![('f', "..".into(), None)]),
            (
                "write-through escaping relative link",
                vec![
                    ('l', "x".into(), Some("../outside".into())),
                    ('f', "x/passwd".into(), None),
                ],
            ),
            (
                "write-through escaping absolute link",
                vec![
                    ('l', "x".into(), Some("/etc".into())),
                    ('f', "x/passwd".into(), None),
                ],
            ),
            (
                "write-through chain of two links",
                vec![
                    ('l', "l1".into(), Some("../out1".into())),
                    ('l', "l2".into(), Some("l1".into())),
                    ('f', "l2/passwd".into(), None),
                ],
            ),
            (
                "hardlink target outside the archive",
                vec![('h', "y".into(), Some("../sibling".into()))],
            ),
            ("link without a target", vec![('l', "z".into(), None)]),
            (
                "symlink cycle",
                vec![
                    ('l', "a".into(), Some("b".into())),
                    ('l', "b".into(), Some("a".into())),
                    ('f', "a/f".into(), None),
                ],
            ),
        ];
        let mut failures: Vec<String> = Vec::new();
        for (name, case) in &bad {
            if RootfsManager::validate_members(case).is_ok() {
                failures.push(format!("{name} was ACCEPTED"));
            }
        }
        assert!(failures.is_empty(), "hostile shapes leaked: {failures:?}");
    }

    #[test]
    fn validate_members_accepts_real_rootfs_shapes() {
        type M = (char, String, Option<String>);
        // Measured against real distro images (review S1-a): Alpine has
        // 307/339 absolute symlink targets and 25 containing '..'; Debian
        // slim 185 absolute. All of these MUST pass.
        let ok: Vec<M> = vec![
            ('d', "./".into(), None),
            ('d', "usr/bin/".into(), None),
            ('f', "usr/bin/busybox".into(), None),
            // The canonical dir-merge symlink.
            ('l', "bin".into(), Some("usr/bin".into())),
            // Absolute applet links (Alpine's whole model).
            ('l', "bin/ls".into(), Some("/bin/busybox".into())),
            // '..' targets that stay net-inside.
            ('l', "var/run".into(), Some("../run".into())),
            (
                'l',
                "var/spool/cron/crontabs".into(),
                Some("../../../etc/crontabs".into()),
            ),
            // A member written THROUGH a net-inside link is fine.
            ('f', "var/run/daemon.pid".into(), None),
            ('f', "etc/os-release".into(), None),
            // A hardlink to an in-root sibling.
            ('h', "usr/bin/less".into(), Some("usr/bin/more".into())),
        ];
        assert!(RootfsManager::validate_members(&ok).is_ok());
        assert!(RootfsManager::validate_members(&[]).is_ok());
    }

    // End-to-end with a REAL crafted archive: a hostile member must be
    // caught by the pre-scan, and a clean archive must pass it.
    #[test]
    fn validate_tarball_scans_real_archives() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let hostile = tmp.path().join("hostile.tar.gz");
        {
            let file = std::fs::File::create(&hostile).unwrap();
            let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut ar = tar::Builder::new(gz);
            let mut header = tar::Header::new_gnu();
            header.set_size(0);
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_mode(0o777);
            let data: &[u8] = b"";
            header.set_path("x").unwrap();
            header.set_link_name("../outside").unwrap();
            header.set_cksum();
            ar.append(&header, data).unwrap();
            // Isolated link alone is safe; the member under it escapes.
            let mut fheader = tar::Header::new_gnu();
            fheader.set_size(0);
            fheader.set_mode(0o644);
            fheader.set_path("x/passwd").unwrap();
            fheader.set_cksum();
            ar.append(&fheader, data).unwrap();
            ar.into_inner().unwrap().finish().unwrap();
        }
        assert!(RootfsManager::validate_tarball(&hostile).is_err());

        let clean = tmp.path().join("clean.tar.gz");
        {
            let file = std::fs::File::create(&clean).unwrap();
            let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut ar = tar::Builder::new(gz);
            let mut header = tar::Header::new_gnu();
            header.set_size(b"hello".len() as u64);
            header.set_mode(0o644);
            let data: &[u8] = b"hello";
            header.set_path("usr/bin/hello").unwrap();
            header.set_cksum();
            ar.append(&header, data).unwrap();
            ar.into_inner().unwrap().finish().unwrap();
        }
        assert!(RootfsManager::validate_tarball(&clean).is_ok());
    }

    /// WIRING pin (mutation-killed the day it was written): the pre-scan
    /// must sit inside `extract_rootfs`, not just exist as a function —
    /// removing the call lets GNU tar extract the hostile archive below.
    #[tokio::test]
    async fn extract_rootfs_refuses_hostile_tarball_before_tar_runs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let hostile = tmp.path().join("wiring.tar.gz");
        {
            let file = std::fs::File::create(&hostile).unwrap();
            let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut ar = tar::Builder::new(gz);
            let mut header = tar::Header::new_gnu();
            header.set_size(0);
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_mode(0o777);
            header.set_path("escape").unwrap();
            header.set_link_name("../outside").unwrap();
            header.set_cksum();
            ar.append(&header, &b""[..]).unwrap();
            // Isolated escaping links are safe; the member UNDER it is the
            // write-through escape this archive must be refused for.
            let mut fheader = tar::Header::new_gnu();
            fheader.set_size(0);
            fheader.set_mode(0o644);
            fheader.set_path("escape/passwd").unwrap();
            fheader.set_cksum();
            ar.append(&fheader, &b""[..]).unwrap();
            ar.into_inner().unwrap().finish().unwrap();
        }
        // NOTE: extract_rootfs short-circuits Ok(()) when dest already
        // exists (pull_image's "already extracted" path) — so the test must
        // NOT pre-create it; the function makes its own directory.
        let dest = tmp.path().join("dest");
        let mgr = RootfsManager::new(tmp.path());
        let result = mgr.extract_rootfs(&hostile, &dest).await;
        assert!(result.is_err(), "pre-scan must refuse the hostile archive");
        assert!(
            !dest.join("escape").exists(),
            "nothing may be extracted from a refused archive"
        );
        assert!(
            !dest.exists(),
            "refusal happens before mkdir — no empty dest left to fake-cache"
        );
    }

    /// SECURITY regression (perfsec 2026-11-14): `base_rootfs_path` used to
    /// pass "." and ".." through the charset replace untouched, so
    /// `remove_image("..")` joined `base_dir/..` — the DATA root — and
    /// recursively deleted it. Every image name must stay INSIDE the cache
    /// root, no matter what it contains.
    #[test]
    fn base_rootfs_path_never_escapes_the_cache_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mgr = RootfsManager::new(tmp.path());
        let root = mgr.cache_dir();
        for evil in [
            "..",
            ".",
            "",
            "a..b",
            "../x",
            "a/../../b",
            "\\\\srv\\share",
            "a\\b",
        ] {
            let p = mgr.base_rootfs_path(evil);
            assert!(
                p.starts_with(root),
                "image name {evil:?} must stay under the cache root, got {p:?}"
            );
        }
        // Review S2 pin: distinct rejected names get DISTINCT sink dirs
        // (one shared constant let a first rejected pull short-circuit
        // every other), and no rejected name shares a directory with a
        // legit mapping.
        let a = mgr.base_rootfs_path("..");
        let b = mgr.base_rootfs_path("../x");
        assert_ne!(a, b, "distinct evil names must not collide");
        assert!(
            !a.to_string_lossy().ends_with("_rejected_image_name_"),
            "no shared-constant sink"
        );
        // S3c pin: the pre-fix code mapped "" to the cache root ITSELF —
        // remove_image("") deleted the whole cache root. Never again.
        assert_ne!(
            mgr.base_rootfs_path(""),
            root,
            "empty name must not resolve to the cache root itself"
        );
    }

    #[test]
    fn validate_rejects_traversal_segments() {
        assert!(RootfsManager::validate_path_component("../../etc", "id").is_err());
        assert!(RootfsManager::validate_path_component("..", "id").is_err());
        assert!(RootfsManager::validate_path_component("a/../../b", "id").is_err());
        assert!(RootfsManager::validate_path_component("", "id").is_err());
        assert!(RootfsManager::validate_path_component("/absolute", "id").is_err());
    }

    #[test]
    fn validate_accepts_safe_identifiers() {
        assert!(RootfsManager::validate_path_component("web-1", "id").is_ok());
        assert!(RootfsManager::validate_path_component("My_App.v2", "id").is_ok());
        assert!(RootfsManager::validate_path_component("9lives", "id").is_ok());
    }

    #[test]
    fn resolve_under_rejects_escape() {
        let dir = std::env::temp_dir().join("cherino-resolve-under-test");
        std::fs::create_dir_all(&dir).unwrap();
        // The escaped candidate must be rejected even though the target
        // does not exist (the walk-up canonicalization handles it).
        let err = RootfsManager::resolve_under(&dir, std::path::Path::new("../../etc/passwd"));
        assert!(err.is_err(), "traversal must be rejected");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_under_accepts_confinement() {
        let dir = std::env::temp_dir().join("cherino-resolve-under-ok");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let ok = RootfsManager::resolve_under(&dir, std::path::Path::new("sub/file.txt"));
        assert!(ok.is_ok(), "confined path must resolve: {:?}", ok.err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
