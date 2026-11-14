use crate::{apparmor, egress::EgressPolicy, seccomp};

pub struct ContainerSecurity {
    pub cap_drop: Option<Vec<String>>,
    pub cap_add: Option<Vec<String>>,
    pub security_opt: Option<Vec<String>>,
    /// Immutable container rootfs (`readonly_rootfs`). `Some(true)` hardens the
    /// sandbox rootfs; scratch space is provided by tmpfs mounts (see
    /// [`apply_readonly_scratch`]). `None` leaves the writable rootfs default
    /// (infrastructure containers whose images write into system paths).
    pub read_only_rootfs: Option<bool>,
    pub egress_policy: Option<EgressPolicy>,
}

/// tmpfs scratch paths layered onto a container whenever the rootfs is made
/// read-only, so standard toolchains (compilers, package managers, shells)
/// keep a writable scratch area without weakening rootfs immutability.
///
/// - `/tmp`, `/var/tmp`: POSIX scratch conventions.
/// - `/run`: pidfiles / sockets created by images that expect a tmpfs `/run`.
///
/// tmpfs pages are charged to the container cgroup, so the existing memory
/// limit caps scratch usage.
pub const READONLY_SCRATCH_PATHS: [&str; 3] = ["/tmp", "/var/tmp", "/run"];

/// Merge [`READONLY_SCRATCH_PATHS`] into a Docker `HostConfig` tmpfs map,
/// preserving any caller-supplied tmpfs entries (caller options win on key
/// collision).
#[cfg(feature = "docker")]
pub fn apply_readonly_scratch(host_config: &mut bollard::service::HostConfig) {
    let mut tmpfs = host_config.tmpfs.take().unwrap_or_default();
    for path in READONLY_SCRATCH_PATHS {
        tmpfs.entry(path.to_string()).or_default();
    }
    host_config.tmpfs = Some(tmpfs);
}

pub fn postgres() -> ContainerSecurity {
    ContainerSecurity {
        cap_drop: Some(vec![]),
        cap_add: None,
        security_opt: Some(vec!["no-new-privileges:true".to_string()]),
        // PostgreSQL writes into its data directory (a dedicated volume); the
        // image itself also touches system paths during initdb, so the rootfs
        // stays writable.
        read_only_rootfs: None,
        egress_policy: Some(EgressPolicy::deny_all()),
    }
}

pub fn scepter() -> ContainerSecurity {
    ContainerSecurity {
        cap_drop: Some(vec!["ALL".to_string()]),
        cap_add: Some(vec![
            "NET_BIND_SERVICE".to_string(),
            "SYS_ADMIN".to_string(),
        ]),
        // docker-default's AppArmor profile denies FUSE mounts (fuse-overlayfs)
        // inside the container, which cosmos sub-container rootfs isolation
        // needs. Use the named FUSE-capable profile (not apparmor=unconfined),
        // with a DEV-ONLY unconfined fallback via CHERINO_APPARMOR_UNCONFINED.
        //
        // NO seccomp filter here, deliberately: this container is the
        // orchestrator. It talks to the Docker socket and, in local (L1) mode,
        // is the process that creates the inner OCI sandbox — so it needs the
        // whole mount/namespace family *plus* whatever youki/libcontainer does
        // next (setns into the new namespaces, pivot_root, cgroup mounts). A
        // deny-by-default filter that admits all of that is close to no filter,
        // while adding a new way for the platform's control plane to fail; the
        // agent workload was the actual exposure and it is now filtered in
        // [`cosmos`]. This exception is stated in the boundary table of
        // entelecheia/docs/{en,zh-Hans}/designs/security.md and tracked as a
        // follow-up (a scepter-specific profile needs a privileged test lane,
        // not a blind allowlist).
        security_opt: Some(apparmor::fuse_security_opts()),
        // The scepter orchestrator is long-lived infrastructure: its image and
        // runtime helpers may write into system paths, so keep the rootfs
        // writable (the *inner* cosmos sandboxes are the hardened layer).
        read_only_rootfs: None,
        egress_policy: Some(EgressPolicy::entelecheia_default()),
    }
}

pub fn scepter_readonly() -> ContainerSecurity {
    ContainerSecurity {
        cap_drop: Some(vec!["ALL".to_string()]),
        cap_add: Some(vec!["NET_BIND_SERVICE".to_string()]),
        security_opt: None,
        read_only_rootfs: None,
        egress_policy: Some(EgressPolicy::entelecheia_default()),
    }
}

/// `security_opt` for the cosmos preset: the named FUSE AppArmor profile **and**
/// the cosmos seccomp filter.
///
/// Both are load-bearing and neither is optional. seccomp decides which
/// syscalls exist (deny-by-default, with the two classic-mount syscalls the
/// in-container FUSE rootfs probe needs re-allowed — see
/// [`seccomp::SeccompProfileData::cosmos`]); AppArmor decides which mounts are
/// permitted (`fstype=fuse.*`, `overlay`, …). seccomp cannot
/// scope `mount(2)` because its `fstype`/target arguments are pointers, so
/// shipping seccomp without the AppArmor profile would be unconfined mounting,
/// and shipping AppArmor without seccomp (the state before this change) left
/// `ptrace`/`bpf`/`kexec_load` reachable. [`verify_mount_mediation`] enforces
/// the pairing on any caller-supplied list.
///
/// The pair is asserted at container-creation time by
/// [`verify_effective_security_opts`].
pub fn cosmos_security_opts() -> Vec<String> {
    merge_security_opts(
        apparmor::fuse_security_opts(),
        seccomp::build_security_opts(Some(seccomp::SeccompProfile::Cosmos)),
    )
}

/// Concatenates two `security_opt` lists, keeping the first entry per key
/// (`no-new-privileges` is emitted by both helpers).
fn merge_security_opts(primary: Vec<String>, secondary: Vec<String>) -> Vec<String> {
    let mut merged = primary;
    for opt in secondary {
        let key = security_opt_key(&opt);
        if !merged
            .iter()
            .any(|existing| security_opt_key(existing) == key)
        {
            merged.push(opt);
        }
    }
    merged
}

/// The key of one Docker `security_opt` entry: the text before the first `=` or
/// `:` (`seccomp={...}`, `apparmor=…`, `no-new-privileges:true`).
pub fn security_opt_key(opt: &str) -> &str {
    opt.split(['=', ':']).next().unwrap_or(opt).trim()
}

/// The value of one `security_opt` entry: everything after the first `=` or `:`,
/// trimmed, or `""` when the entry carries no separator.
///
/// Separator-based on purpose: slicing by the key's length would be wrong for an
/// entry with leading whitespace and panic on a non-ASCII key.
fn security_opt_value(opt: &str) -> &str {
    match opt.find(['=', ':']) {
        Some(at) => opt[at + 1..].trim(),
        None => "",
    }
}

/// One requested `security_opt` entry the live container does not carry as
/// requested.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SecurityOptDrift {
    /// Keys absent from the container entirely.
    pub missing: Vec<String>,
    /// Keys present with a different value.
    pub mismatched: Vec<String>,
    /// Keys that appear more than once on either side: which one the runtime
    /// applies is not defined, so the boundary is not what the list says.
    pub duplicated: Vec<String>,
}

impl SecurityOptDrift {
    pub fn is_empty(&self) -> bool {
        self.missing.is_empty() && self.mismatched.is_empty() && self.duplicated.is_empty()
    }

    /// A one-line description that names keys only — the seccomp entry is a
    /// multi-kilobyte JSON document and must never be echoed whole.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.missing.is_empty() {
            parts.push(format!("missing {}", self.missing.join(", ")));
        }
        if !self.mismatched.is_empty() {
            parts.push(format!("mismatched {}", self.mismatched.join(", ")));
        }
        if !self.duplicated.is_empty() {
            parts.push(format!("duplicated {}", self.duplicated.join(", ")));
        }
        parts.join("; ")
    }
}

/// The cosmos filter admits `mount`, and seccomp cannot scope `mount(2)`'s
/// target — so that half is only safe together with a named AppArmor profile.
///
/// Returns `Err(reason)` when a `security_opt` list pairs a seccomp profile that
/// admits `mount` with no `apparmor=` entry, or with `apparmor=unconfined`.
/// That combination is one call away (`build_security_opts(Some(Cosmos))` is
/// exactly how `compile()` builds its list) and it is the difference between a
/// scoped boundary and unconfined mounting, so it is checked rather than
/// assumed. Lists whose seccomp profile denies `mount` (the default and
/// `compile` presets) are fine on their own.
pub fn verify_mount_mediation(requested: &[String]) -> Result<(), String> {
    // Duplicated keys make the list ambiguous: which entry a runtime applies is
    // not defined, so nothing about it is provable.
    for key in requested.iter().map(|opt| security_opt_key(opt)) {
        if requested
            .iter()
            .filter(|opt| security_opt_key(opt) == key)
            .count()
            > 1
        {
            return Err(format!(
                "`{key}` appears more than once in security_opt; which entry the runtime                  applies is undefined, so the boundary is unprovable"
            ));
        }
    }
    // A named AppArmor profile is the thing that scopes `mount(2)`'s target, so
    // its presence is sufficient and its absence is what must be disproved.
    let named_apparmor = requested
        .iter()
        .find(|opt| security_opt_key(opt) == "apparmor")
        .map(|opt| security_opt_value(opt))
        .is_some_and(|name| !name.is_empty() && name != "unconfined");
    if named_apparmor {
        return Ok(());
    }
    // Fail closed: without AppArmor, the only acceptable list is one whose
    // seccomp profile **provably denies** `mount`. "Provably" means a refusing
    // default action *and* no `SCMP_ACT_ALLOW` rule naming `mount` — a profile
    // with `defaultAction: SCMP_ACT_ALLOW` and no rules denies nothing, so it
    // must not be read as proof. Anything we cannot parse — `seccomp=unconfined`,
    // a profile path, no seccomp entry at all — is unproven, and unproven means
    // refused.
    let denies_mount = requested
        .iter()
        .find(|opt| security_opt_key(opt) == "seccomp")
        .and_then(|opt| serde_json::from_str::<serde_json::Value>(security_opt_value(opt)).ok())
        .is_some_and(|profile| {
            let refusing_default = profile["defaultAction"].as_str().is_some_and(|action| {
                matches!(
                    action,
                    "SCMP_ACT_ERRNO"
                        | "SCMP_ACT_KILL"
                        | "SCMP_ACT_KILL_THREAD"
                        | "SCMP_ACT_KILL_PROCESS"
                        | "SCMP_ACT_TRAP"
                )
            });
            // A grant is *any* rule naming `mount` whose action does not refuse:
            // `SCMP_ACT_ALLOW` obviously, but also `SCMP_ACT_LOG` (allow and log),
            // `SCMP_ACT_TRACE` and `SCMP_ACT_NOTIFY`. Recognising only ALLOW
            // certifies "provably denies mount" for a profile that lets the call
            // through while logging it.
            let refuses = |action: &str| {
                matches!(
                    action,
                    "SCMP_ACT_ERRNO"
                        | "SCMP_ACT_KILL"
                        | "SCMP_ACT_KILL_THREAD"
                        | "SCMP_ACT_KILL_PROCESS"
                        | "SCMP_ACT_TRAP"
                )
            };
            let allows_mount = profile["syscalls"].as_array().is_some_and(|rules| {
                rules.iter().any(|rule| {
                    rule["names"]
                        .as_array()
                        .is_some_and(|names| names.iter().any(|name| name == "mount"))
                        && !refuses(rule["action"].as_str().unwrap_or_default())
                })
            });
            refusing_default && !allows_mount
        });
    if denies_mount {
        return Ok(());
    }
    Err(
        "mount is not scoped: no named AppArmor profile is present and the seccomp entry \
         does not provably deny `mount` (unconfined, a profile path, a filter that admits \
         mount, or no filter at all)"
            .to_string(),
    )
}

/// Whether a capability set makes `mount(2)` reachable at all.
///
/// `CAP_SYS_ADMIN` is the one that matters; `ALL` is the wildcard form. Anything
/// else (including the file-ownership caps the compile preset adds) cannot mount.
pub fn grants_mount_capability(cap_add: Option<&[String]>) -> bool {
    cap_add.is_some_and(|caps| {
        caps.iter().any(|cap| {
            // The runtime normalises capability spellings before it applies them
            // (case-insensitive, `CAP_` prefix optional) — measured on Docker
            // 29.6 here: `SYS_ADMIN`, `CAP_SYS_ADMIN`, `sys_admin` and
            // `Cap_Sys_Admin` all produce the same effective mask. A guard that
            // exact-matched one spelling would read "cannot mount" for the others
            // and wave the request through with real SYS_ADMIN granted.
            let upper = cap.trim().to_ascii_uppercase();
            let name = upper.strip_prefix("CAP_").unwrap_or(upper.as_str());
            name == "SYS_ADMIN" || name == "ALL"
        })
    })
}

/// Which DEV-ONLY escape hatches are open in this process.
///
/// Passed in rather than read from the environment so the decision below stays
/// unit-testable; [`DevHatches::from_env`] is the production source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DevHatches {
    /// `DISABLE_SECCOMP` is set: the request carries no seccomp filter.
    pub seccomp_disabled: bool,
    /// `CHERINO_APPARMOR_UNCONFINED` (or the legacy name) is set: the request
    /// carries `apparmor=unconfined`.
    pub apparmor_unconfined: bool,
}

impl DevHatches {
    pub fn from_env() -> Self {
        Self {
            seccomp_disabled: crate::seccomp::is_seccomp_disabled(),
            apparmor_unconfined: crate::apparmor::is_apparmor_unconfined(),
        }
    }

    fn any(self) -> Option<&'static str> {
        if self.seccomp_disabled {
            Some("DISABLE_SECCOMP")
        } else if self.apparmor_unconfined {
            Some("CHERINO_APPARMOR_UNCONFINED")
        } else {
            None
        }
    }
}

/// What a create path must do with a caller's `security_opt`, decided without a
/// container runtime so the wiring has an always-run test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestDecision {
    /// A scoped boundary; `hatch` names the DEV-ONLY escape hatch that made it
    /// pass despite unproven mount mediation, when one did.
    Proceed { hatch: Option<&'static str> },
    /// Not a scoped boundary: nothing may be created.
    Refuse(String),
}

/// The pre-create half of the security self-check.
///
/// `grants_mount_capability` says whether the container will hold
/// `CAP_SYS_ADMIN` (or `ALL`): without it, `mount(2)` cannot succeed whatever the
/// filter allows, so there is nothing to scope and the request proceeds — a
/// `postgres()`-style list (only `no-new-privileges`) must not be refused just
/// because it names neither AppArmor nor a seccomp profile.
///
/// With the capability granted, a request whose `mount` is unproven is refused —
/// unless a DEV-ONLY hatch is deliberately open, in which case it proceeds with
/// the hatch named so the caller can make the downgrade loud. The hatches exist
/// to unblock local work; silently bricking them would be worse than a warning,
/// because the next person would remove the guard instead of the hatch.
pub fn decide_security_request(
    requested: &[String],
    hatches: DevHatches,
    grants_mount_capability: bool,
) -> RequestDecision {
    if !grants_mount_capability {
        return RequestDecision::Proceed { hatch: None };
    }
    match verify_mount_mediation(requested) {
        Ok(()) => RequestDecision::Proceed { hatch: None },
        Err(reason) => match hatches.any() {
            Some(hatch) => RequestDecision::Proceed { hatch: Some(hatch) },
            None => RequestDecision::Refuse(reason),
        },
    }
}

/// Checks that every requested `security_opt` entry is actually in effect on the
/// container the runtime created.
///
/// This is the startup self-check: a runtime that silently drops, rewrites or
/// ignores a security option downgrades the sandbox without failing, and the
/// difference is invisible from inside the container once it is running. The
/// comparison is by key, and values are compared JSON-wise for `seccomp` so that
/// a re-serialized profile still matches; extra entries on the container are
/// allowed (supersets are not downgrades).
pub fn verify_effective_security_opts(
    requested: &[String],
    actual: &[String],
) -> Result<(), SecurityOptDrift> {
    let mut drift = SecurityOptDrift::default();
    for side in [requested, actual] {
        for key in side.iter().map(|opt| security_opt_key(opt)) {
            let seen = side
                .iter()
                .filter(|opt| security_opt_key(opt) == key)
                .count();
            if seen > 1 && !drift.duplicated.iter().any(|k| k == key) {
                drift.duplicated.push(key.to_string());
            }
        }
    }
    for want in requested {
        let key = security_opt_key(want);
        match actual.iter().find(|have| security_opt_key(have) == key) {
            None => drift.missing.push(key.to_string()),
            Some(have) if !opt_values_match(key, want, have) => {
                drift.mismatched.push(key.to_string())
            }
            Some(_) => {}
        }
    }
    if drift.is_empty() { Ok(()) } else { Err(drift) }
}

fn opt_values_match(key: &str, requested: &str, actual: &str) -> bool {
    let want = security_opt_value(requested);
    let have = security_opt_value(actual);
    if want == have {
        return true;
    }
    if key != "seccomp" {
        return false;
    }
    // Docker resolves `seccomp=/path/to/profile.json` (or a named profile) into
    // the profile itself, so only an inline JSON request can be compared field
    // by field; anything else is presence-checked.
    match (
        serde_json::from_str::<serde_json::Value>(want),
        serde_json::from_str::<serde_json::Value>(have),
    ) {
        (Ok(want), Ok(have)) => want == have,
        (Err(_), _) => true,
        (Ok(_), Err(_)) => false,
    }
}

pub fn cosmos() -> ContainerSecurity {
    ContainerSecurity {
        // Drop ALL then add back the minimal set cosmos sub-containers need:
        // - NET_BIND_SERVICE: bind to ports < 1024 (if scepter binds low)
        // - SYS_ADMIN: needed if sub-container probes fuse/overlay at startup
        // - CHOWN / FOWNER / SETGID / SETUID: file ownership on shared volumes
        cap_drop: Some(vec!["ALL".to_string()]),
        cap_add: Some(vec![
            "NET_BIND_SERVICE".to_string(),
            "SYS_ADMIN".to_string(),
            "CHOWN".to_string(),
            "FOWNER".to_string(),
            "SETGID".to_string(),
            "SETUID".to_string(),
            "DAC_OVERRIDE".to_string(),
        ]),
        // The cosmos filter: deny-by-default with the in-container FUSE probe's
        // two classic-mount syscalls re-allowed, paired with the named FUSE-capable AppArmor
        // profile that decides *which* mounts are permitted. Never one without
        // the other — see [`cosmos_security_opts`].
        security_opt: Some(cosmos_security_opts()),
        // Ephemeral agent sandbox: hardened by default. The rootfs is an
        // immutable snapshot (fork = copy-on-write snapshot of the rootfs);
        // all legitimate writes go to workspace volumes or the tmpfs scratch
        // paths (see [`READONLY_SCRATCH_PATHS`]). A compromised agent cannot
        // persist changes into its own image layer.
        read_only_rootfs: Some(true),
        egress_policy: Some(EgressPolicy::entelecheia_default()),
    }
}

pub fn compile() -> ContainerSecurity {
    ContainerSecurity {
        // Drop ALL then add back the minimal set the compile container needs.
        // The compile container runs build toolchains (rustc/cargo, npm, go,
        // …) as uid 1000:1000 writing to shared cache/workspace volumes, so it
        // needs the same file-ownership capabilities cosmos uses — but NOT
        // SYS_ADMIN (it does not orchestrate sub-containers, so there is no
        // fuse-overlayfs mount) and NOT NET_BIND_SERVICE (build servers bind
        // high ports, never below 1024).
        // - CHOWN / FOWNER / SETGID / SETUID / DAC_OVERRIDE: ownership and
        //   permissions on shared volumes owned by the host/root.
        cap_drop: Some(vec!["ALL".to_string()]),
        cap_add: Some(vec![
            "CHOWN".to_string(),
            "FOWNER".to_string(),
            "SETGID".to_string(),
            "SETUID".to_string(),
            "DAC_OVERRIDE".to_string(),
        ]),
        security_opt: Some(seccomp::build_security_opts(Some(
            seccomp::SeccompProfile::Compile,
        ))),
        // Compile containers write toolchain state (target dirs, caches) into
        // the caller-provided volumes; the image layer stays writable for now
        // because some toolchains touch system paths (e.g. /etc/passwd lookups
        // under sccache wrappers). Hardening them is tracked separately.
        read_only_rootfs: None,
        // Compile containers legitimately need open network: they fetch
        // dependencies from arbitrary package registries (crates.io, npm, pypi,
        // Maven Central, …) and clone git dependencies, which cannot be
        // enumerated ahead of time. Leaving this `None` defers to the caller
        // (snowflake_manager layers its own egress policy plus per-workspace
        // registry domains on top), and the Docker path falls back to the
        // `entelecheia_default` DNS-only soft egress.
        egress_policy: None,
    }
}

#[cfg(feature = "docker")]
pub fn apply_to_host_config(
    sec: &ContainerSecurity,
    host_config: &mut bollard::service::HostConfig,
) {
    let default_egress = EgressPolicy::entelecheia_default();
    let egress = sec.egress_policy.as_ref().unwrap_or(&default_egress);
    egress.apply_to_docker_config(host_config);

    if let Some(ref dns) = host_config.dns
        && dns.is_empty()
    {
        host_config.dns = None;
    }

    host_config.cap_drop = Some(
        sec.cap_drop
            .clone()
            .unwrap_or_else(|| vec!["ALL".to_string()]),
    );
    host_config.cap_add = sec.cap_add.clone();
    host_config.security_opt = Some(
        sec.security_opt
            .clone()
            .unwrap_or_else(|| seccomp::build_security_opts(None)),
    );
    if sec.read_only_rootfs == Some(true) {
        host_config.readonly_rootfs = Some(true);
        apply_readonly_scratch(host_config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result, ensure};

    #[test]
    fn postgres_no_cap_drop() -> Result<()> {
        let sec = postgres();
        assert_eq!(sec.cap_drop, Some(vec![]));
        assert!(
            sec.egress_policy
                .as_ref()
                .context("egress policy expected")?
                .is_restricted()
        );
        Ok(())
    }

    #[test]
    fn scepter_restricts_caps_and_egress() -> Result<()> {
        let sec = scepter();
        assert_eq!(sec.cap_drop, Some(vec!["ALL".to_string()]));
        let added = sec.cap_add.as_ref().context("cap_add expected")?;
        assert!(
            added.contains(&"SYS_ADMIN".to_string()),
            "must have SYS_ADMIN for container orchestration"
        );
        assert!(added.contains(&"NET_BIND_SERVICE".to_string()));
        assert!(!added.contains(&"ALL".to_string()), "must not wildcard-add");
        // scepter uses the named FUSE AppArmor profile (needed for fuse-overlayfs)
        assert!(sec.security_opt.is_some());
        assert!(
            sec.egress_policy
                .as_ref()
                .context("egress policy expected")?
                .is_restricted()
        );
        Ok(())
    }

    #[test]
    fn cosmos_uses_functional_profile() -> Result<()> {
        let sec = cosmos();
        // Must explicitly drop ALL then add back needed caps
        assert_eq!(sec.cap_drop, Some(vec!["ALL".to_string()]));
        let added = sec.cap_add.as_ref().context("cap_add expected")?;
        assert!(
            added.contains(&"SYS_ADMIN".to_string()),
            "cosmos needs SYS_ADMIN"
        );
        assert!(added.contains(&"NET_BIND_SERVICE".to_string()));
        // Must have explicit security_opt (not None → strict seccomp)
        assert!(sec.security_opt.is_some());
        // Must have egress filtering
        assert!(
            sec.egress_policy
                .as_ref()
                .context("egress policy expected")?
                .is_restricted()
        );
        Ok(())
    }

    #[test]
    fn compile_drops_all_caps_and_keeps_only_file_ownership() -> Result<()> {
        let sec = compile();
        // Must drop ALL then add back the minimal file-ownership set.
        assert_eq!(sec.cap_drop, Some(vec!["ALL".to_string()]));
        let added = sec.cap_add.as_ref().context("cap_add expected")?;
        assert!(added.contains(&"CHOWN".to_string()));
        assert!(added.contains(&"FOWNER".to_string()));
        assert!(added.contains(&"SETGID".to_string()));
        assert!(added.contains(&"SETUID".to_string()));
        assert!(added.contains(&"DAC_OVERRIDE".to_string()));
        // Must never re-add the privileged caps reserved for orchestration.
        assert!(!added.contains(&"SYS_ADMIN".to_string()));
        assert!(!added.contains(&"NET_BIND_SERVICE".to_string()));
        assert!(!added.contains(&"ALL".to_string()), "must not wildcard-add");
        assert!(sec.security_opt.is_some());
        // Compile containers need open network for package registries; the
        // caller layers registry domains on top.
        assert!(sec.egress_policy.is_none());
        Ok(())
    }

    #[cfg(feature = "docker")]
    #[test]
    fn apply_compile_to_host_config_drops_all_and_adds_file_caps() -> Result<()> {
        let sec = compile();
        let mut hc = bollard::service::HostConfig::default();
        apply_to_host_config(&sec, &mut hc);
        assert_eq!(hc.cap_drop, Some(vec!["ALL".to_string()]));
        let added = hc.cap_add.as_ref().context("cap_add expected")?;
        assert!(added.contains(&"CHOWN".to_string()));
        assert!(added.contains(&"DAC_OVERRIDE".to_string()));
        assert!(!added.contains(&"SYS_ADMIN".to_string()));
        let sec_opt = hc.security_opt.context("security_opt expected")?;
        assert!(
            sec_opt.iter().any(|o| o.starts_with("seccomp=")),
            "compile profile must embed a seccomp profile"
        );
        Ok(())
    }

    #[cfg(feature = "docker")]
    #[test]
    fn apply_postgres_to_host_config() -> Result<()> {
        let sec = postgres();
        let mut hc = bollard::service::HostConfig::default();
        apply_to_host_config(&sec, &mut hc);
        assert_eq!(hc.cap_drop, Some(vec![]));
        assert!(hc.security_opt.is_some());
        assert_eq!(hc.dns, Some(vec!["0.0.0.0".to_string()]));
        Ok(())
    }

    #[cfg(feature = "docker")]
    #[test]
    fn apply_scepter_to_host_config() -> Result<()> {
        let sec = scepter();
        let mut hc = bollard::service::HostConfig::default();
        apply_to_host_config(&sec, &mut hc);
        assert_eq!(hc.cap_drop, Some(vec!["ALL".to_string()]));
        let added = hc.cap_add.as_ref().context("cap_add expected")?;
        assert!(
            added.contains(&"SYS_ADMIN".to_string()),
            "scepter needs SYS_ADMIN"
        );
        let sec_opt = hc.security_opt.context("security_opt expected")?;
        assert!(
            sec_opt
                .iter()
                .any(|o| o == &format!("apparmor={}", apparmor::installed_profile_name())),
            "scepter uses the named FUSE AppArmor profile for fuse-overlayfs"
        );
        assert!(
            sec_opt.iter().any(|o| o == "no-new-privileges:true"),
            "scepter must keep no-new-privileges"
        );
        assert!(hc.dns.is_some());
        Ok(())
    }

    #[cfg(feature = "docker")]
    #[test]
    fn apply_cosmos_to_host_config() -> Result<()> {
        let sec = cosmos();
        let mut hc = bollard::service::HostConfig::default();
        apply_to_host_config(&sec, &mut hc);
        assert_eq!(hc.cap_drop, Some(vec!["ALL".to_string()]));
        let added = hc.cap_add.as_ref().context("cap_add expected")?;
        assert!(added.contains(&"SYS_ADMIN".to_string()));
        let sec_opt = hc.security_opt.context("security_opt expected")?;
        assert!(
            sec_opt
                .iter()
                .any(|o| o == &format!("apparmor={}", apparmor::installed_profile_name())),
            "cosmos uses the named FUSE AppArmor profile"
        );
        assert!(
            sec_opt.iter().any(|o| o.starts_with("seccomp=")),
            "cosmos carries the cosmos seccomp filter (closed 2026-11-15)"
        );
        assert!(hc.dns.is_some());
        Ok(())
    }

    #[test]
    fn scepter_drops_all_then_adds_orchestrator_caps() -> Result<()> {
        let sec = scepter();
        ensure!(
            sec.cap_drop == Some(vec!["ALL".to_string()]),
            "Scepter must drop ALL capabilities first"
        );
        let added = sec.cap_add.as_ref().context("cap_add expected")?;
        ensure!(
            added.contains(&"SYS_ADMIN".to_string()),
            "Scepter must have SYS_ADMIN for fuse-overlayfs fallback"
        );
        ensure!(
            !added.iter().any(|c| c == "ALL" || c == "*" || c.is_empty()),
            "Scepter must not wildcard-add capabilities"
        );
        Ok(())
    }

    #[test]
    fn scepter_readonly_has_no_sys_admin() -> Result<()> {
        let sec = scepter_readonly();
        if let Some(ref caps) = sec.cap_add {
            ensure!(
                !caps.iter().any(|c| c == "SYS_ADMIN"),
                "Scepter read-only profile must never have SYS_ADMIN"
            );
        }
        ensure!(
            sec.cap_drop == Some(vec!["ALL".to_string()]),
            "Scepter read-only must drop ALL capabilities first"
        );
        Ok(())
    }

    #[test]
    fn scepter_has_restricted_egress() -> Result<()> {
        let sec = scepter();
        ensure!(
            sec.egress_policy
                .as_ref()
                .context("egress policy expected")?
                .is_restricted(),
            "Scepter must use restricted egress policy"
        );
        Ok(())
    }

    #[cfg(feature = "docker")]
    #[test]
    fn all_profiles_drop_or_override_caps() -> Result<()> {
        // postgres and scepter_readonly must drop caps and never add SYS_ADMIN
        for (name, sec) in [
            ("postgres", postgres()),
            ("scepter_readonly", scepter_readonly()),
        ] {
            let mut hc = bollard::service::HostConfig::default();
            apply_to_host_config(&sec, &mut hc);
            ensure!(
                hc.cap_drop.is_some(),
                "{} profile must explicitly drop capabilities",
                name
            );
            if let Some(ref added) = hc.cap_add {
                ensure!(
                    !added.iter().any(|c| c == "SYS_ADMIN"),
                    "{} profile must never add SYS_ADMIN",
                    name
                );
            }
        }
        Ok(())
    }

    #[cfg(feature = "docker")]
    #[test]
    fn all_profiles_have_security_opts() -> Result<()> {
        // All profiles must produce some security_opt after apply_to_host_config
        for (name, sec) in [
            ("scepter", scepter()),
            ("cosmos", cosmos()),
            ("compile", compile()),
        ] {
            let mut hc = bollard::service::HostConfig::default();
            apply_to_host_config(&sec, &mut hc);
            let sec_opt = hc.security_opt.unwrap_or_default();
            ensure!(
                !sec_opt.is_empty(),
                "{} profile must produce some security_opt",
                name
            );
        }
        Ok(())
    }

    #[cfg(feature = "docker")]
    #[test]
    fn postgres_has_no_new_privileges() -> Result<()> {
        let sec = postgres();
        let mut hc = bollard::service::HostConfig::default();
        apply_to_host_config(&sec, &mut hc);
        let sec_opt = hc.security_opt.unwrap_or_default();
        ensure!(
            sec_opt.iter().any(|o| o.contains("no-new-privileges")),
            "postgres profile must have no-new-privileges"
        );
        Ok(())
    }

    #[test]
    fn cosmos_hardens_rootfs_by_default() -> Result<()> {
        let sec = cosmos();
        ensure!(
            sec.read_only_rootfs == Some(true),
            "cosmos preset must default to a read-only rootfs"
        );
        Ok(())
    }

    #[test]
    fn infrastructure_presets_keep_writable_rootfs() -> Result<()> {
        for (name, sec) in [
            ("postgres", postgres()),
            ("scepter", scepter()),
            ("scepter_readonly", scepter_readonly()),
            ("compile", compile()),
        ] {
            ensure!(
                sec.read_only_rootfs.is_none(),
                "{} preset must not force a read-only rootfs",
                name
            );
        }
        Ok(())
    }

    #[test]
    fn cosmos_security_opts_carry_apparmor_and_seccomp() -> Result<()> {
        let opts = cosmos_security_opts();
        ensure!(
            opts.iter()
                .any(|o| o == &format!("apparmor={}", apparmor::installed_profile_name())),
            "the named FUSE AppArmor profile must be present: {:?}",
            opts.iter().map(|o| security_opt_key(o)).collect::<Vec<_>>()
        );
        ensure!(
            opts.iter().any(|o| o.starts_with("seccomp=")),
            "the cosmos seccomp filter must be present: {:?}",
            opts.iter().map(|o| security_opt_key(o)).collect::<Vec<_>>()
        );
        ensure!(
            opts.iter()
                .any(|o| security_opt_key(o) == "no-new-privileges"),
            "no-new-privileges must survive the merge"
        );
        Ok(())
    }

    #[test]
    fn cosmos_seccomp_entry_is_the_cosmos_profile_json() -> Result<()> {
        let opts = cosmos_security_opts();
        let seccomp = opts
            .iter()
            .find(|o| o.starts_with("seccomp="))
            .context("seccomp entry expected")?;
        let value = security_opt_value(seccomp);
        let parsed: serde_json::Value = serde_json::from_str(value)?;
        let allowed: Vec<&str> = parsed["syscalls"]
            .as_array()
            .context("syscalls")?
            .iter()
            .filter(|rule| rule["action"] == "SCMP_ACT_ALLOW")
            .flat_map(|rule| rule["names"].as_array().into_iter().flatten())
            .filter_map(|n| n.as_str())
            .collect();
        ensure!(
            allowed.contains(&"mount"),
            "cosmos seccomp must allow mount"
        );
        ensure!(
            !allowed.contains(&"unshare"),
            "the namespace family left with the OCI half: unshare must not be allowed"
        );
        let denied: Vec<&str> = parsed["syscalls"]
            .as_array()
            .context("syscalls")?
            .iter()
            .filter(|rule| rule["action"] == "SCMP_ACT_ERRNO")
            .flat_map(|rule| rule["names"].as_array().into_iter().flatten())
            .filter_map(|n| n.as_str())
            .collect();
        ensure!(denied.contains(&"ptrace"), "ptrace must stay denied");
        ensure!(denied.contains(&"bpf"), "bpf must stay denied");
        ensure!(
            denied.contains(&"kexec_load"),
            "kexec_load must stay denied"
        );
        Ok(())
    }

    #[test]
    fn merge_security_opts_keeps_one_entry_per_key() -> Result<()> {
        let merged = merge_security_opts(
            vec![
                "no-new-privileges:true".to_string(),
                "apparmor=celestia-cherino-fuse".to_string(),
            ],
            vec![
                "no-new-privileges:true".to_string(),
                "seccomp={\"defaultAction\":\"SCMP_ACT_ERRNO\"}".to_string(),
            ],
        );
        ensure!(
            merged
                .iter()
                .filter(|o| security_opt_key(o) == "no-new-privileges")
                .count()
                == 1,
            "no-new-privileges must not be duplicated: {merged:?}"
        );
        ensure!(merged.len() == 3, "exactly three distinct keys: {merged:?}");
        Ok(())
    }

    #[test]
    fn cosmos_security_opts_pass_the_mount_mediation_guard() -> Result<()> {
        ensure!(
            verify_mount_mediation(&cosmos_security_opts()).is_ok(),
            "the cosmos pair must satisfy its own guard"
        );
        Ok(())
    }

    #[test]
    fn a_mount_capable_filter_without_apparmor_is_refused() -> Result<()> {
        // Exactly how `compile()` assembles its list: the seccomp entry alone.
        let lone = seccomp::build_security_opts(Some(seccomp::SeccompProfile::Cosmos));
        let reason = verify_mount_mediation(&lone)
            .expect_err("seccomp-with-mount and no AppArmor must be refused");
        ensure!(
            reason.contains("AppArmor"),
            "the reason must name what is missing: {reason}"
        );
        Ok(())
    }

    #[test]
    fn apparmor_unconfined_does_not_mediate_mounts() -> Result<()> {
        let mut opts = seccomp::build_security_opts(Some(seccomp::SeccompProfile::Cosmos));
        opts.push("apparmor=unconfined".to_string());
        ensure!(
            verify_mount_mediation(&opts).is_err(),
            "unconfined AppArmor leaves mount(2) unscoped and must be refused"
        );
        Ok(())
    }

    #[test]
    fn a_filter_that_denies_mount_needs_no_apparmor() -> Result<()> {
        for profile in [
            seccomp::SeccompProfile::EntelecheiaDefault,
            seccomp::SeccompProfile::Compile,
        ] {
            ensure!(
                verify_mount_mediation(&seccomp::build_security_opts(Some(profile))).is_ok(),
                "{profile:?} denies mount and must pass on its own"
            );
        }
        Ok(())
    }

    #[test]
    fn unproven_seccomp_without_apparmor_is_refused() -> Result<()> {
        // Fail closed: `unconfined`, a profile path, and "no filter at all" all
        // leave mount(2) unscoped and none of them may pass.
        for opts in [
            vec!["seccomp=unconfined".to_string()],
            vec![
                "seccomp=unconfined".to_string(),
                "apparmor=unconfined".to_string(),
            ],
            vec!["seccomp=/etc/docker/cosmos.json".to_string()],
            vec!["no-new-privileges:true".to_string()],
            vec![],
        ] {
            ensure!(
                verify_mount_mediation(&opts).is_err(),
                "unproven mount mediation must be refused: {opts:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_named_apparmor_scopes_mount_even_with_an_unconfined_seccomp() -> Result<()> {
        let opts = vec![
            "seccomp=unconfined".to_string(),
            "apparmor=celestia-cherino-fuse".to_string(),
        ];
        ensure!(
            verify_mount_mediation(&opts).is_ok(),
            "a named AppArmor profile is what scopes mount(2)"
        );
        Ok(())
    }

    #[test]
    fn any_non_refusing_action_naming_mount_is_a_grant() -> Result<()> {
        // `SCMP_ACT_LOG` allows and logs; recognising only SCMP_ACT_ALLOW would
        // certify "provably denies mount" for a profile that permits the call.
        for action in ["SCMP_ACT_LOG", "SCMP_ACT_TRACE", "SCMP_ACT_NOTIFY"] {
            let profile = format!(
                r#"{{"defaultAction":"SCMP_ACT_ERRNO","architectures":[],"syscalls":[{{"names":["mount"],"action":"{action}"}}]}}"#
            );
            ensure!(
                verify_mount_mediation(&[format!("seccomp={profile}")]).is_err(),
                "{action} naming mount must not count as a denial"
            );
        }
        // And the explicit-ALLOW shape from the guard's own threat model.
        let granted = r#"{"defaultAction":"SCMP_ACT_ERRNO","architectures":[],"syscalls":[{"names":["mount","umount2"],"action":"SCMP_ACT_ALLOW"}]}"#;
        ensure!(
            verify_mount_mediation(&[format!("seccomp={granted}")]).is_err(),
            "a refusing default with an explicit ALLOW rule for mount is a grant"
        );
        // A refusing rule for mount is a denial, not a grant.
        let denied = r#"{"defaultAction":"SCMP_ACT_ERRNO","architectures":[],"syscalls":[{"names":["mount"],"action":"SCMP_ACT_ERRNO"}]}"#;
        ensure!(
            verify_mount_mediation(&[format!("seccomp={denied}")]).is_ok(),
            "an explicit refusal of mount is proof"
        );
        Ok(())
    }

    #[test]
    fn capability_spellings_are_normalised() -> Result<()> {
        // Measured on Docker 29.6: all of these grant the same effective mask, so
        // the guard must read all of them as "can mount".
        for spelling in [
            "SYS_ADMIN",
            "CAP_SYS_ADMIN",
            "sys_admin",
            "Cap_Sys_Admin",
            "CAP_ALL",
            "all",
        ] {
            ensure!(
                grants_mount_capability(Some(&[spelling.to_string()])),
                "{spelling} must be read as mount capability"
            );
        }
        for spelling in ["CHOWN", "CAP_CHOWN", "NET_BIND_SERVICE", "DAC_OVERRIDE"] {
            ensure!(
                !grants_mount_capability(Some(&[spelling.to_string()])),
                "{spelling} must not be read as mount capability"
            );
        }
        Ok(())
    }

    #[test]
    fn a_default_allow_profile_is_not_proof_of_denial() -> Result<()> {
        // `defaultAction: SCMP_ACT_ALLOW` with no rules denies nothing; reading
        // "no ALLOW rule names mount" as proof would wave it through.
        let permissive = r#"{"defaultAction":"SCMP_ACT_ALLOW","architectures":[],"syscalls":[]}"#;
        let opts = vec![format!("seccomp={permissive}")];
        ensure!(
            verify_mount_mediation(&opts).is_err(),
            "a default-allow profile must not count as denying mount"
        );
        // A refusing default with no mount grant is the real proof.
        let refusing = r#"{"defaultAction":"SCMP_ACT_ERRNO","architectures":[],"syscalls":[]}"#;
        ensure!(
            verify_mount_mediation(&[format!("seccomp={refusing}")]).is_ok(),
            "a refusing default action with no mount grant is proof"
        );
        Ok(())
    }

    #[test]
    fn duplicated_keys_make_the_boundary_unprovable() -> Result<()> {
        let mut opts = cosmos_security_opts();
        opts.push("seccomp=unconfined".to_string());
        ensure!(
            verify_mount_mediation(&opts).is_err(),
            "two seccomp entries mean the applied one is undefined"
        );
        Ok(())
    }

    #[test]
    fn decide_security_request_proceeds_on_the_cosmos_pair() -> Result<()> {
        ensure!(
            decide_security_request(&cosmos_security_opts(), DevHatches::default(), true)
                == RequestDecision::Proceed { hatch: None },
            "the shipped pair must proceed without a hatch"
        );
        Ok(())
    }

    #[test]
    fn decide_security_request_refuses_a_mispaired_list() -> Result<()> {
        let mut opts = seccomp::build_security_opts(Some(seccomp::SeccompProfile::Cosmos));
        opts.push("apparmor=unconfined".to_string());
        ensure!(
            matches!(
                decide_security_request(&opts, DevHatches::default(), true),
                RequestDecision::Refuse(_)
            ),
            "an unproven list must be refused without a hatch"
        );
        Ok(())
    }

    #[test]
    fn a_container_without_sys_admin_needs_no_mount_mediation() -> Result<()> {
        // The `postgres()` preset: only `no-new-privileges`, no SYS_ADMIN, no
        // seccomp, no AppArmor. It cannot mount, so nothing needs scoping —
        // demanding mediation here would fail every such create.
        let mut params_caps: Option<Vec<String>> = None;
        ensure!(
            !grants_mount_capability(params_caps.as_deref()),
            "no cap_add means no mount capability"
        );
        let postgres_opts = vec!["no-new-privileges:true".to_string()];
        ensure!(
            decide_security_request(&postgres_opts, DevHatches::default(), false)
                == RequestDecision::Proceed { hatch: None },
            "a non-mounting container must proceed"
        );
        // With SYS_ADMIN the same list is a real hole and must be refused.
        ensure!(
            matches!(
                decide_security_request(&postgres_opts, DevHatches::default(), true),
                RequestDecision::Refuse(_)
            ),
            "the same list with SYS_ADMIN must be refused"
        );
        params_caps = Some(vec!["CHOWN".to_string(), "SETGID".to_string()]);
        ensure!(
            !grants_mount_capability(params_caps.as_deref()),
            "file-ownership caps are not mount capability"
        );
        ensure!(grants_mount_capability(Some(&["SYS_ADMIN".to_string()])));
        ensure!(grants_mount_capability(Some(&["ALL".to_string()])));
        Ok(())
    }

    #[test]
    fn decide_security_request_honours_the_dev_hatches() -> Result<()> {
        // The hatches exist to unblock local work; a silently bricked hatch is
        // how someone deletes the guard at 3am, so they proceed — named, so the
        // caller can make the downgrade loud.
        let unpaired = vec!["no-new-privileges:true".to_string()];
        for (hatches, expected) in [
            (
                DevHatches {
                    seccomp_disabled: true,
                    apparmor_unconfined: false,
                },
                "DISABLE_SECCOMP",
            ),
            (
                DevHatches {
                    seccomp_disabled: false,
                    apparmor_unconfined: true,
                },
                "CHERINO_APPARMOR_UNCONFINED",
            ),
        ] {
            ensure!(
                decide_security_request(&unpaired, hatches, true)
                    == RequestDecision::Proceed {
                        hatch: Some(expected)
                    },
                "hatch {expected} must let the request through, named"
            );
        }
        Ok(())
    }

    #[test]
    fn duplicated_keys_are_drift() -> Result<()> {
        let requested = cosmos_security_opts();
        let mut actual = requested.clone();
        actual.push("seccomp=unconfined".to_string());
        let drift = verify_effective_security_opts(&requested, &actual)
            .expect_err("two seccomp entries mean the applied one is undefined");
        ensure!(
            drift.duplicated == vec!["seccomp".to_string()],
            "the drift must name the duplicated key: {drift:?}"
        );
        Ok(())
    }

    #[test]
    fn security_opt_value_handles_whitespace_and_non_ascii_keys() -> Result<()> {
        // Regression: slicing by the key's byte length panicked on a non-ASCII
        // key with leading whitespace and returned a wrong value otherwise.
        ensure!(security_opt_value("  seccomp={\"a\":1}") == "{\"a\":1}");
        ensure!(security_opt_value("apparmor=celestia-cherino-fuse") == "celestia-cherino-fuse");
        ensure!(security_opt_value("no-new-privileges:true") == "true");
        ensure!(security_opt_value("privéleged") == "");
        Ok(())
    }

    #[test]
    fn verify_security_opts_accepts_a_reformatted_seccomp_profile() -> Result<()> {
        // A runtime that re-serializes the profile (key order, whitespace) is
        // not a downgrade; the check must not fail on formatting.
        let requested = cosmos_security_opts();
        let actual: Vec<String> = requested
            .iter()
            .map(|opt| {
                if security_opt_key(opt) == "seccomp" {
                    let value: serde_json::Value =
                        serde_json::from_str(security_opt_value(opt)).unwrap();
                    format!("seccomp={}", serde_json::to_string(&value).unwrap())
                } else {
                    opt.clone()
                }
            })
            .chain(std::iter::once("label=extra".to_string()))
            .collect();
        ensure!(
            verify_effective_security_opts(&requested, &actual).is_ok(),
            "reformatted profile plus an extra opt must verify"
        );
        Ok(())
    }

    #[test]
    fn verify_security_opts_flags_the_pre_change_cosmos_state() -> Result<()> {
        // The negative control for the detector: the state observed on the live
        // sandbox before this change — AppArmor + no-new-privileges, no seccomp.
        let requested = cosmos_security_opts();
        let actual = vec![
            "no-new-privileges:true".to_string(),
            format!("apparmor={}", apparmor::installed_profile_name()),
        ];
        let drift = verify_effective_security_opts(&requested, &actual)
            .expect_err("a container without seccomp must be reported as drift");
        ensure!(
            drift.missing == vec!["seccomp".to_string()],
            "the drift must name seccomp: {drift:?}"
        );
        ensure!(
            drift.summary().contains("seccomp") && !drift.summary().contains("defaultAction"),
            "the summary names keys only, never the profile body"
        );
        Ok(())
    }

    #[test]
    fn verify_security_opts_flags_a_different_apparmor_profile() -> Result<()> {
        let requested = cosmos_security_opts();
        let actual: Vec<String> = requested
            .iter()
            .map(|opt| {
                if security_opt_key(opt) == "apparmor" {
                    "apparmor=unconfined".to_string()
                } else {
                    opt.clone()
                }
            })
            .collect();
        let drift = verify_effective_security_opts(&requested, &actual)
            .expect_err("a downgraded AppArmor profile must be reported");
        ensure!(
            drift.mismatched == vec!["apparmor".to_string()],
            "the drift must name apparmor as mismatched: {drift:?}"
        );
        Ok(())
    }

    #[test]
    fn verify_security_opts_flags_a_missing_full_security_opt_set() -> Result<()> {
        let requested = cosmos_security_opts();
        let drift = verify_effective_security_opts(&requested, &[])
            .expect_err("no security opts at all must be reported");
        ensure!(
            drift.missing.contains(&"seccomp".to_string())
                && drift.missing.contains(&"apparmor".to_string()),
            "both load-bearing keys must be reported: {drift:?}"
        );
        Ok(())
    }

    #[test]
    fn verify_security_opts_accepts_a_resolved_seccomp_path() -> Result<()> {
        // Docker resolves `seccomp=<path>` into the profile; presence is the
        // strongest claim available then.
        let requested = vec!["seccomp=/etc/docker/cosmos.json".to_string()];
        let actual = vec![format!(
            "seccomp={}",
            seccomp::SeccompProfile::Cosmos.to_json_string()?
        )];
        ensure!(
            verify_effective_security_opts(&requested, &actual).is_ok(),
            "a path request resolved to a profile must verify"
        );
        Ok(())
    }

    #[cfg(feature = "docker")]
    #[test]
    fn apply_cosmos_sets_readonly_and_scratch_tmpfs() -> Result<()> {
        let sec = cosmos();
        let mut hc = bollard::service::HostConfig::default();
        apply_to_host_config(&sec, &mut hc);
        ensure!(
            hc.readonly_rootfs == Some(true),
            "cosmos profile must set readonly_rootfs"
        );
        let tmpfs = hc.tmpfs.context("cosmos profile must set tmpfs scratch")?;
        for path in READONLY_SCRATCH_PATHS {
            ensure!(
                tmpfs.contains_key(path),
                "scratch path {} must be a tmpfs mount",
                path
            );
        }
        Ok(())
    }

    #[cfg(feature = "docker")]
    #[test]
    fn apply_scepter_leaves_rootfs_writable() -> Result<()> {
        let sec = scepter();
        let mut hc = bollard::service::HostConfig::default();
        apply_to_host_config(&sec, &mut hc);
        ensure!(
            hc.readonly_rootfs.is_none(),
            "scepter profile must not flip readonly_rootfs"
        );
        ensure!(hc.tmpfs.is_none(), "scepter profile must not add tmpfs");
        Ok(())
    }

    #[cfg(feature = "docker")]
    #[test]
    fn readonly_scratch_preserves_caller_tmpfs_options() -> Result<()> {
        let mut hc = bollard::service::HostConfig::default();
        let mut caller = std::collections::HashMap::new();
        caller.insert("/tmp".to_string(), "size=64m".to_string());
        hc.tmpfs = Some(caller);
        apply_readonly_scratch(&mut hc);
        let tmpfs = hc.tmpfs.context("tmpfs expected")?;
        ensure!(
            tmpfs.get("/tmp").map(String::as_str) == Some("size=64m"),
            "caller-provided tmpfs options must win"
        );
        ensure!(
            tmpfs.contains_key("/var/tmp") && tmpfs.contains_key("/run"),
            "missing scratch paths must be filled in"
        );
        Ok(())
    }
}
