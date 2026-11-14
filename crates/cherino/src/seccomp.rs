//! Seccomp syscall-filter profile construction for container sandboxes.
//!
//! [`SeccompProfile`] describes a deny-by-default syscall filter (default
//! action `SCMP_ACT_ERRNO`) with an explicit allow-list of the syscalls a
//! sandboxed container legitimately needs. The filter is embedded as a
//! `seccomp=...` Docker `security_opt`, or converted to the OCI `LinuxSeccomp`
//! spec by the `container_runtime` crate.
//!
//! # Profiles
//!
//! [`SeccompProfile::EntelecheiaDefault`] is the workload default: no `mount`,
//! no kernel surface. [`SeccompProfile::Cosmos`] is the same filter with the two
//! syscalls the in-container FUSE rootfs probe needs re-allowed — see
//! [`SeccompProfileData::cosmos`]. Note what the default does **not** claim:
//! `clone`/`clone3` are allow-listed without argument filtering, so namespace
//! *creation* via clone flags is reachable (the boundary there is AppArmor and
//! the absence of `mount`, not this list); only the standalone `unshare(2)` is
//! denied by default.
//!
//! # Layering with AppArmor
//!
//! The two mechanisms answer different questions and both are load-bearing:
//! seccomp decides **which syscalls** exist for the process; AppArmor
//! (`crate::apparmor`, profile `celestia-cherino-fuse`) decides **which mounts**
//! are permitted (`fstype=fuse.*`, `overlay`, `proc`, `sysfs`, `cgroup`, plus
//! docker-default's remount rules). A seccomp rule cannot express "only FUSE
//! mounts": the filter may only compare scalar arguments and `mount(2)`'s
//! `fstype`/`target` are pointers. Allowing `mount` here without the matching
//! AppArmor profile would be unconfined mounting, so the two profiles are
//! always applied together (see `security_profile::cosmos`).
//!
//! # DEV-ONLY escape hatch
//!
//! The `DISABLE_SECCOMP` environment variable (see [`is_seccomp_disabled`])
//! disables the filter entirely and MUST NEVER be set in production.

use serde::{Deserialize, Serialize};

pub fn default_seccomp_security_opt() -> String {
    "no-new-privileges:true".to_string()
}

/// Returns true when the `DISABLE_SECCOMP` escape hatch is enabled.
///
/// # DEV-ONLY escape hatch — never set in production
///
/// `DISABLE_SECCOMP=true` (or `1`) makes [`build_security_opts`] omit the
/// seccomp syscall filter entirely, leaving the container protected only by the
/// runtime's default policy (if any). The deny-by-default filter is what blocks
/// `ptrace`, `mount`, `bpf`, `kexec_load`, `unshare`, and other escape vectors,
/// so this variable MUST NEVER be set in production. It exists only to unblock
/// local development when a container image needs a syscall the profile does not
/// yet allow.
pub fn is_seccomp_disabled() -> bool {
    let disabled = seccomp_disabled_from_value(std::env::var("DISABLE_SECCOMP").ok().as_deref());

    if disabled {
        tracing::warn!(
            "DISABLE_SECCOMP is set: the seccomp syscall filter is DISABLED. \
             This is a DEV-ONLY escape hatch and must never be enabled in production."
        );
    }

    disabled
}

fn seccomp_disabled_from_value(value: Option<&str>) -> bool {
    matches!(value, Some(v) if v.eq_ignore_ascii_case("true") || v == "1")
}

pub fn build_security_opts(profile: Option<SeccompProfile>) -> Vec<String> {
    build_security_opts_with_disabled(profile, is_seccomp_disabled())
}

fn build_security_opts_with_disabled(
    profile: Option<SeccompProfile>,
    seccomp_disabled: bool,
) -> Vec<String> {
    let mut opts = Vec::new();

    opts.push("no-new-privileges:true".to_string());

    if !seccomp_disabled {
        let profile = profile.unwrap_or_default();
        if let Ok(json) = profile.to_json_string() {
            opts.push(format!("seccomp={}", json));
        }
    }

    opts
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeccompSyscallRule {
    pub names: Vec<String>,
    pub action: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum SeccompProfile {
    #[default]
    EntelecheiaDefault,
    Compile,
    /// The cosmos sandbox preset: the default filter with the in-container
    /// two classic-mount syscalls re-allowed. See [`SeccompProfileData::cosmos`].
    Cosmos,
}

impl SeccompProfile {
    pub fn to_profile_data(&self) -> SeccompProfileData {
        match self {
            Self::EntelecheiaDefault => SeccompProfileData::entelecheia_default(),
            Self::Compile => SeccompProfileData::compile(),
            Self::Cosmos => SeccompProfileData::cosmos(),
        }
    }

    pub fn to_json_string(&self) -> serde_json::Result<String> {
        self.to_profile_data().to_json_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeccompProfileData {
    #[serde(rename = "defaultAction")]
    pub default_action: String,
    pub architectures: Vec<String>,
    pub syscalls: Vec<SeccompSyscallRule>,
}

impl SeccompProfileData {
    pub fn entelecheia_default() -> Self {
        Self {
            default_action: "SCMP_ACT_ERRNO".to_string(),
            architectures: vec![
                "SCMP_ARCH_X86_64".to_string(),
                "SCMP_ARCH_X86".to_string(),
                "SCMP_ARCH_AARCH64".to_string(),
                "SCMP_ARCH_ARM".to_string(),
            ],
            syscalls: vec![
                SeccompSyscallRule {
                    names: vec![
                        "accept".into(),
                        "accept4".into(),
                        "access".into(),
                        "arch_prctl".into(),
                        "bind".into(),
                        "brk".into(),
                        "capget".into(),
                        "capset".into(),
                        "chdir".into(),
                        "chmod".into(),
                        "chown".into(),
                        "chown32".into(),
                        "clock_getres".into(),
                        "clock_gettime".into(),
                        "clock_nanosleep".into(),
                        "clone".into(),
                        "clone3".into(),
                        "close".into(),
                        "connect".into(),
                        "copy_file_range".into(),
                        "creat".into(),
                        "dup".into(),
                        "dup2".into(),
                        "dup3".into(),
                        "epoll_create".into(),
                        "epoll_create1".into(),
                        "epoll_ctl".into(),
                        "epoll_pwait".into(),
                        "epoll_wait".into(),
                        "eventfd".into(),
                        "eventfd2".into(),
                        "execve".into(),
                        "execveat".into(),
                        "exit".into(),
                        "exit_group".into(),
                        "faccessat".into(),
                        "faccessat2".into(),
                        "fadvise64".into(),
                        "fallocate".into(),
                        "fanotify_mark".into(),
                        "fchdir".into(),
                        "fchmod".into(),
                        "fchmodat".into(),
                        "fchown".into(),
                        "fchown32".into(),
                        "fchownat".into(),
                        "fcntl".into(),
                        "fcntl64".into(),
                        "fdatasync".into(),
                        "fgetxattr".into(),
                        "flistxattr".into(),
                        "flock".into(),
                        "fork".into(),
                        "fremovexattr".into(),
                        "fsetxattr".into(),
                        "fstat".into(),
                        "fstat64".into(),
                        "fstatat64".into(),
                        "fstatfs".into(),
                        "fstatfs64".into(),
                        "fsync".into(),
                        "ftruncate".into(),
                        "ftruncate64".into(),
                        "futex".into(),
                        "futimesat".into(),
                        "getcwd".into(),
                        "getdents".into(),
                        "getdents64".into(),
                        "getegid".into(),
                        "getegid32".into(),
                        "geteuid".into(),
                        "geteuid32".into(),
                        "getgid".into(),
                        "getgid32".into(),
                        "getgroups".into(),
                        "getitimer".into(),
                        "getpeername".into(),
                        "getpgrp".into(),
                        "getpid".into(),
                        "getppid".into(),
                        "getpriority".into(),
                        "getrandom".into(),
                        "getresgid".into(),
                        "getresgid32".into(),
                        "getresuid".into(),
                        "getresuid32".into(),
                        "getrlimit".into(),
                        "getsockname".into(),
                        "getsockopt".into(),
                        "gettid".into(),
                        "gettimeofday".into(),
                        "getuid".into(),
                        "getuid32".into(),
                        "getxattr".into(),
                        "inotify_add_watch".into(),
                        "inotify_init".into(),
                        "inotify_init1".into(),
                        "inotify_rm_watch".into(),
                        "io_cancel".into(),
                        "io_destroy".into(),
                        "io_getevents".into(),
                        "io_setup".into(),
                        "io_submit".into(),
                        "ioctl".into(),
                        "ioprio_get".into(),
                        "ioprio_set".into(),
                        "ipc".into(),
                        "kill".into(),
                        "lchown".into(),
                        "lchown32".into(),
                        "lgetxattr".into(),
                        "link".into(),
                        "linkat".into(),
                        "listen".into(),
                        "listxattr".into(),
                        "llistxattr".into(),
                        "lremovexattr".into(),
                        "lseek".into(),
                        "lsetxattr".into(),
                        "lstat".into(),
                        "lstat64".into(),
                        "madvise".into(),
                        "memfd_create".into(),
                        "mincore".into(),
                        "mkdir".into(),
                        "mkdirat".into(),
                        "mknod".into(),
                        "mknodat".into(),
                        "mlock".into(),
                        "mlock2".into(),
                        "mlockall".into(),
                        "mmap".into(),
                        "mmap2".into(),
                        "mprotect".into(),
                        "mremap".into(),
                        "msgctl".into(),
                        "msgget".into(),
                        "msgrcv".into(),
                        "msgsnd".into(),
                        "msync".into(),
                        "munlock".into(),
                        "munlockall".into(),
                        "munmap".into(),
                        "nanosleep".into(),
                        "newfstatat".into(),
                        "open".into(),
                        "openat".into(),
                        "openat2".into(),
                        "pipe".into(),
                        "pipe2".into(),
                        "poll".into(),
                        "ppoll".into(),
                        "prctl".into(),
                        "pread64".into(),
                        "preadv".into(),
                        "preadv2".into(),
                        "prlimit64".into(),
                        "pselect6".into(),
                        "pwrite64".into(),
                        "pwritev".into(),
                        "pwritev2".into(),
                        "read".into(),
                        "readahead".into(),
                        "readlink".into(),
                        "readlinkat".into(),
                        "readv".into(),
                        "recv".into(),
                        "recvfrom".into(),
                        "recvmmsg".into(),
                        "recvmsg".into(),
                        "rename".into(),
                        "renameat".into(),
                        "renameat2".into(),
                        "restart_syscall".into(),
                        "rmdir".into(),
                        "rt_sigaction".into(),
                        "rt_sigprocmask".into(),
                        "rt_sigreturn".into(),
                        "rt_sigsuspend".into(),
                        "rt_sigtimedwait".into(),
                        "sched_getaffinity".into(),
                        "sched_setaffinity".into(),
                        "sched_yield".into(),
                        "seccomp".into(),
                        "select".into(),
                        "semctl".into(),
                        "semget".into(),
                        "semop".into(),
                        "semtimedop".into(),
                        "send".into(),
                        "sendfile".into(),
                        "sendfile64".into(),
                        "sendmmsg".into(),
                        "sendmsg".into(),
                        "sendto".into(),
                        "set_robust_list".into(),
                        "set_tid_address".into(),
                        "setfsgid".into(),
                        "setfsgid32".into(),
                        "setfsuid".into(),
                        "setfsuid32".into(),
                        "setgid".into(),
                        "setgid32".into(),
                        "setgroups".into(),
                        "setitimer".into(),
                        "setpgid".into(),
                        "setpriority".into(),
                        "setregid".into(),
                        "setregid32".into(),
                        "setresgid".into(),
                        "setresgid32".into(),
                        "setresuid".into(),
                        "setresuid32".into(),
                        "setrlimit".into(),
                        "setsid".into(),
                        "setsockopt".into(),
                        "setuid".into(),
                        "setuid32".into(),
                        "setxattr".into(),
                        "shmat".into(),
                        "shmctl".into(),
                        "shmdt".into(),
                        "shmget".into(),
                        "shutdown".into(),
                        "sigaltstack".into(),
                        "signalfd".into(),
                        "signalfd4".into(),
                        "socket".into(),
                        "socketpair".into(),
                        "splice".into(),
                        "stat".into(),
                        "stat64".into(),
                        "statfs".into(),
                        "statfs64".into(),
                        "statx".into(),
                        "symlink".into(),
                        "symlinkat".into(),
                        "sync".into(),
                        "sync_file_range".into(),
                        "syncfs".into(),
                        "sysinfo".into(),
                        "syslog".into(),
                        "tee".into(),
                        "tgkill".into(),
                        "timer_create".into(),
                        "timer_delete".into(),
                        "timer_getoverrun".into(),
                        "timer_gettime".into(),
                        "timer_settime".into(),
                        "timerfd_create".into(),
                        "timerfd_gettime".into(),
                        "timerfd_settime".into(),
                        "times".into(),
                        "tkill".into(),
                        "truncate".into(),
                        "truncate64".into(),
                        "umask".into(),
                        "uname".into(),
                        "unlink".into(),
                        "unlinkat".into(),
                        // `unshare` is intentionally NOT allow-listed here: it
                        // creates new namespaces and becomes an escape vector when
                        // combined with SYS_ADMIN (which cosmos re-adds). Note the
                        // limit of this statement: `clone`/`clone3` are allowed
                        // above without argument filtering, so namespace creation
                        // through clone flags stays reachable — this list is not
                        // a namespace boundary, and the profiles do not claim to
                        // be one.
                        "utime".into(),
                        "utimensat".into(),
                        "utimes".into(),
                        "vfork".into(),
                        "vmsplice".into(),
                        "wait4".into(),
                        "waitid".into(),
                        "waitpid".into(),
                        "write".into(),
                        "writev".into(),
                    ],
                    action: "SCMP_ACT_ALLOW".into(),
                },
                SeccompSyscallRule {
                    names: vec![
                        "ptrace".into(),
                        "mount".into(),
                        "umount2".into(),
                        "kexec_load".into(),
                        "kexec_file_load".into(),
                        "bpf".into(),
                        "reboot".into(),
                        "add_key".into(),
                        "keyctl".into(),
                        "request_key".into(),
                        "init_module".into(),
                        "finit_module".into(),
                        "delete_module".into(),
                        "swapon".into(),
                        "swapoff".into(),
                        "sysfs".into(),
                        "uselib".into(),
                        "acct".into(),
                        "modify_ldt".into(),
                        "personality".into(),
                        "process_vm_readv".into(),
                        "process_vm_writev".into(),
                        "vhangup".into(),
                        "pivot_root".into(),
                        "ioperm".into(),
                        "iopl".into(),
                        "perf_event_open".into(),
                        "userfaultfd".into(),
                    ],
                    action: "SCMP_ACT_ERRNO".into(),
                },
            ],
        }
    }

    pub fn compile() -> Self {
        let mut profile = Self::entelecheia_default();
        if let Some(block_rule) = profile
            .syscalls
            .iter_mut()
            .find(|r| r.action == "SCMP_ACT_ERRNO")
        {
            block_rule
                .names
                .retain(|name| !matches!(name.as_str(), "userfaultfd"));
        }
        profile
    }

    /// The cosmos sandbox filter: [`Self::entelecheia_default`] plus the two
    /// syscalls the in-container FUSE rootfs probe needs
    /// ([`COSMOS_FUSE_PROBE_SYSCALLS`]).
    ///
    /// **One caller justifies this list**: `cherino-runtime`'s
    /// `detect_rootfs_capability` spawns `fuse-overlayfs -o
    /// lowerdir=…,upperdir=…,workdir=… MERGED` (and `mount -t overlay overlay …`
    /// as the kernel-overlay fallback). `fuse-overlayfs` runs as root in the
    /// container, so libfuse issues classic `mount(2)` itself — the setuid
    /// `fusermount3` path is inert under `no-new-privileges`.
    ///
    /// **Not justified, and therefore not here: the inner OCI runtime.** The
    /// Layer-5 sandbox (`libcontainer 0.7`, the crate `cherino-runtime` links)
    /// mounts every filesystem through the fd-based API —
    /// `open_tree`/`mount_setattr`/`move_mount` for binds,
    /// `fsopen`/`fsconfig`/`fsmount`/… otherwise, with no classic fallback
    /// (`libcontainer-0.7.0/src/rootfs/mount.rs`) — and AppArmor does not
    /// mediate that API on kernel 6.1, so admitting it would reopen the
    /// mount-authority bypass this profile exists to close.
    ///
    /// For accuracy about *why* it fails: libcontainer does make classic
    /// `mount(2)` calls, and they run first and succeed — the root propagation
    /// remount (`rootfs/rootfs.rs:52`), the rootfs self-bind
    /// (`rootfs/rootfs.rs:68-75`), `Mount::make_parent_mount_private`
    /// (`rootfs/mount.rs:557`), the `/dev` node binds (`rootfs/device.rs:127`)
    /// and the init-process mounts (`process/init/process.rs:211,523,542,591,707,729`).
    /// Every mount that comes from the OCI spec then goes through the fd-based
    /// path and fails with EPERM. So the failure is *after* the classic calls,
    /// not before them: those calls succeed and are simply not the mounts that
    /// matter. Consequence, stated
    /// as a supported limit: **a cosmos container that hosts the inner OCI
    /// runtime cannot run under this filter** — its first inner mount returns
    /// EPERM (fail-closed, loudly). Reconciling the two needs either a substrate
    /// whose LSM mediates the fd-based API or a runtime that uses classic
    /// `mount(2)`; a `CosmosInnerOci`-style variant is a separate decision with
    /// its own threat model, not a rider on this one.
    ///
    /// What stays denied is the kernel-attack surface no supported caller needs:
    /// `ptrace`, `bpf`, `kexec_load`/`kexec_file_load`, module load/unload, the
    /// keyring calls, `perf_event_open`, `userfaultfd`, `process_vm_readv`/
    /// `writev`, `reboot` and the swap calls. Mount *authority* is not granted:
    /// `mount(2)`'s `fstype` and target are pointers, so a syscall filter cannot
    /// scope them; the AppArmor profile `celestia-cherino-fuse` does
    /// (`mount fstype=fuse.*`, `overlay`, `proc`, `sysfs`, `cgroup`).
    pub fn cosmos() -> Self {
        let mut profile = Self::entelecheia_default();
        for rule in &mut profile.syscalls {
            match rule.action.as_str() {
                "SCMP_ACT_ALLOW" => {
                    for name in COSMOS_FUSE_PROBE_SYSCALLS {
                        insert_sorted(&mut rule.names, name);
                    }
                }
                "SCMP_ACT_ERRNO" => rule
                    .names
                    .retain(|name| !COSMOS_FUSE_PROBE_SYSCALLS.contains(&name.as_str())),
                _ => {}
            }
        }
        profile
    }

    pub fn to_json_string(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

/// The syscalls the in-container FUSE rootfs probe needs, and that a plain
/// workload does not.
///
/// Re-allowed for cosmos containers **only**. Both are mediated by AppArmor on
/// the classic path (measured: `mount(2)` of `tmpfs` under
/// `celestia-cherino-fuse` returns `EACCES`, while `fstype=fuse.*`/`overlay`
/// succeed — see [`SeccompProfileData::cosmos`]), so this grants the syscall,
/// not the authority.
///
/// **The fd-based mount API is deliberately absent** — `fsopen`, `fsconfig`,
/// `fsmount`, `move_mount`, `open_tree`, `mount_setattr` are not AppArmor-
/// mediated on kernel 6.1 (measured 2026-11-15: classic `mount(2)` of tmpfs is
/// refused with EACCES under the installed profile while
/// `fsopen`→`fsconfig(CREATE)`→`fsmount`→`move_mount` of the same tmpfs succeeds
/// and appears in `/proc/mounts`, and `open_tree(OPEN_TREE_CLONE)` of `/etc`
/// succeeds). Their absence is also why the inner OCI runtime is unsupported
/// here — `libcontainer 0.7` mounts only through that API.
pub const COSMOS_FUSE_PROBE_SYSCALLS: &[&str] = &["mount", "umount2"];

/// Inserts a name at its sorted position, keeping an allow-list readable and
/// making a re-run idempotent.
fn insert_sorted(names: &mut Vec<String>, name: &str) {
    if names.iter().any(|existing| existing == name) {
        return;
    }
    let at = names
        .iter()
        .position(|existing| existing.as_str() > name)
        .unwrap_or(names.len());
    names.insert(at, name.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result, ensure};

    #[test]
    fn seccomp_profile_generates_valid_json() -> Result<()> {
        let profile = SeccompProfile::EntelecheiaDefault;
        let json = profile.to_json_string()?;
        assert!(json.contains("SCMP_ACT_ERRNO"));
        assert!(json.contains("SCMP_ACT_ALLOW"));
        assert!(json.contains("ptrace"));
        assert!(json.contains("open"));
        assert!(json.contains("read"));
        assert!(json.contains("write"));
        Ok(())
    }

    #[test]
    fn seccomp_profile_allows_basic_syscalls() -> Result<()> {
        let profile = SeccompProfile::EntelecheiaDefault.to_profile_data();
        let allow_rule = &profile.syscalls[0];
        assert_eq!(allow_rule.action, "SCMP_ACT_ALLOW");
        assert!(allow_rule.names.contains(&"read".to_string()));
        assert!(allow_rule.names.contains(&"write".to_string()));
        assert!(allow_rule.names.contains(&"open".to_string()));
        assert!(allow_rule.names.contains(&"close".to_string()));
        assert!(allow_rule.names.contains(&"fstat".to_string()));
        assert!(allow_rule.names.contains(&"mmap".to_string()));
        assert!(allow_rule.names.contains(&"brk".to_string()));
        assert!(allow_rule.names.contains(&"futex".to_string()));
        Ok(())
    }

    #[test]
    fn seccomp_profile_blocks_dangerous_syscalls() -> Result<()> {
        let profile = SeccompProfile::EntelecheiaDefault.to_profile_data();
        let block_rule = &profile.syscalls[1];
        assert_eq!(block_rule.action, "SCMP_ACT_ERRNO");
        assert!(block_rule.names.contains(&"ptrace".to_string()));
        assert!(block_rule.names.contains(&"mount".to_string()));
        assert!(block_rule.names.contains(&"kexec_load".to_string()));
        assert!(block_rule.names.contains(&"bpf".to_string()));
        assert!(block_rule.names.contains(&"reboot".to_string()));
        Ok(())
    }

    #[test]
    fn seccomp_profile_has_4_architectures() -> Result<()> {
        let profile = SeccompProfile::EntelecheiaDefault.to_profile_data();
        assert_eq!(profile.architectures.len(), 4);
        Ok(())
    }

    #[test]
    fn build_security_opts_includes_no_new_privileges() -> Result<()> {
        let opts = build_security_opts(None);
        assert!(opts.contains(&"no-new-privileges:true".to_string()));
        Ok(())
    }

    #[test]
    fn build_security_opts_includes_seccomp_profile() -> Result<()> {
        let opts = build_security_opts(None);
        let has_seccomp = opts.iter().any(|o| o.starts_with("seccomp="));
        ensure!(
            has_seccomp,
            "security opts should include seccomp profile: {:?}",
            opts
        );
        Ok(())
    }

    #[test]
    fn build_security_opts_merges_additional() -> Result<()> {
        let opts = build_security_opts(None);
        assert!(opts.contains(&"no-new-privileges:true".to_string()));
        Ok(())
    }

    #[test]
    fn build_security_opts_no_duplicates() -> Result<()> {
        let opts = build_security_opts(None);
        let count = opts
            .iter()
            .filter(|o| **o == "no-new-privileges:true")
            .count();
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    fn seccomp_profile_present_by_default() -> Result<()> {
        let opts = build_security_opts_with_disabled(None, false);
        ensure!(
            opts.iter().any(|o| o.starts_with("seccomp=")),
            "seccomp profile must be present when not disabled: {:?}",
            opts
        );
        Ok(())
    }

    #[test]
    fn seccomp_profile_absent_only_when_explicitly_disabled() -> Result<()> {
        let opts = build_security_opts_with_disabled(None, true);
        ensure!(
            !opts.iter().any(|o| o.starts_with("seccomp=")),
            "seccomp profile must be absent only when DISABLE_SECCOMP is enabled: {:?}",
            opts
        );
        ensure!(
            opts.contains(&"no-new-privileges:true".to_string()),
            "no-new-privileges must remain even when seccomp is disabled"
        );
        Ok(())
    }

    #[test]
    fn seccomp_disabled_parses_only_truthy_values() -> Result<()> {
        assert!(seccomp_disabled_from_value(Some("true")));
        assert!(seccomp_disabled_from_value(Some("TRUE")));
        assert!(seccomp_disabled_from_value(Some("1")));
        assert!(!seccomp_disabled_from_value(Some("false")));
        assert!(!seccomp_disabled_from_value(Some("0")));
        assert!(!seccomp_disabled_from_value(Some("")));
        assert!(!seccomp_disabled_from_value(None));
        Ok(())
    }

    #[test]
    fn unshare_not_allow_or_deny_listed() -> Result<()> {
        // For a plain workload `unshare` is an escape vector: it must appear in
        // NEITHER the allow-list NOR the explicit deny-list, relying on the
        // deny-by-default action (SCMP_ACT_ERRNO) to block it. The cosmos preset
        // is the documented exception (it hosts namespace-creating code) and is
        // asserted separately below.
        for profile in [
            SeccompProfile::EntelecheiaDefault.to_profile_data(),
            SeccompProfile::Compile.to_profile_data(),
        ] {
            let allow_rule = profile
                .syscalls
                .iter()
                .find(|r| r.action == "SCMP_ACT_ALLOW")
                .context("allow rule expected")?;
            let deny_rule = profile
                .syscalls
                .iter()
                .find(|r| r.action == "SCMP_ACT_ERRNO")
                .context("deny rule expected")?;
            ensure!(
                !allow_rule.names.contains(&"unshare".to_string()),
                "unshare must not be allow-listed"
            );
            ensure!(
                !deny_rule.names.contains(&"unshare".to_string()),
                "unshare must not be explicitly deny-listed (rely on default deny)"
            );
        }
        Ok(())
    }

    fn rule<'a>(profile: &'a SeccompProfileData, action: &str) -> Result<&'a SeccompSyscallRule> {
        profile
            .syscalls
            .iter()
            .find(|r| r.action == action)
            .with_context(|| format!("{action} rule expected"))
    }

    #[test]
    fn cosmos_profile_allows_exactly_the_fuse_probe_syscalls() -> Result<()> {
        let profile = SeccompProfile::Cosmos.to_profile_data();
        let allow = rule(&profile, "SCMP_ACT_ALLOW")?;
        let deny = rule(&profile, "SCMP_ACT_ERRNO")?;
        for name in COSMOS_FUSE_PROBE_SYSCALLS.iter().copied() {
            ensure!(
                allow.names.iter().any(|n| n == name),
                "cosmos profile must allow {name}"
            );
            ensure!(
                !deny.names.iter().any(|n| n == name),
                "{name} must not also be deny-listed (first-match-wins runtimes would deny it)"
            );
        }
        Ok(())
    }

    #[test]
    fn cosmos_profile_adds_exactly_mount_and_umount2() -> Result<()> {
        // Pinned literally, not through the const: a test that reads the same
        // list it is validating can never notice the list changing.
        let default = SeccompProfile::EntelecheiaDefault.to_profile_data();
        let cosmos = SeccompProfile::Cosmos.to_profile_data();
        let allow = rule(&cosmos, "SCMP_ACT_ALLOW")?;
        let default_allow = rule(&default, "SCMP_ACT_ALLOW")?;
        let added: Vec<&str> = allow
            .names
            .iter()
            .map(String::as_str)
            .filter(|name| !default_allow.names.iter().any(|n| n == name))
            .collect();
        ensure!(
            added == vec!["mount", "umount2"],
            "the cosmos preset must add exactly [mount, umount2], got {added:?}"
        );
        Ok(())
    }

    #[test]
    fn cosmos_profile_keeps_the_kernel_escape_surface_denied() -> Result<()> {
        // The point of the whole change: the probe's classic mounts become reachable,
        // and the kernel-attack surface stays shut.
        let profile = SeccompProfile::Cosmos.to_profile_data();
        let allow = rule(&profile, "SCMP_ACT_ALLOW")?;
        let deny = rule(&profile, "SCMP_ACT_ERRNO")?;
        for name in [
            "ptrace",
            "bpf",
            "kexec_load",
            "kexec_file_load",
            "init_module",
            "finit_module",
            "delete_module",
            "add_key",
            "keyctl",
            "request_key",
            "perf_event_open",
            "userfaultfd",
            "process_vm_readv",
            "process_vm_writev",
            "reboot",
            "swapon",
            "swapoff",
            // The fd-based mount API: not AppArmor-mediated on kernel 6.1, so it
            // must stay on the deny-by-default floor even though it is a "mount".
            // (It is also why the inner OCI runtime is unsupported here.)
            "fsopen",
            "fsconfig",
            "fsmount",
            "move_mount",
            "open_tree",
            "mount_setattr",
            // The namespace family went away with the OCI half: nothing in the
            // supported configuration creates namespaces in-container.
            "unshare",
            "setns",
            "pivot_root",
            "chroot",
            "sethostname",
        ] {
            ensure!(
                !allow.names.iter().any(|n| n == name),
                "{name} must never be allow-listed in the cosmos profile"
            );
        }
        for name in [
            "ptrace",
            "bpf",
            "kexec_load",
            "kexec_file_load",
            "init_module",
            "perf_event_open",
            "userfaultfd",
            "process_vm_readv",
        ] {
            ensure!(
                deny.names.iter().any(|n| n == name),
                "{name} must stay explicitly deny-listed"
            );
        }
        Ok(())
    }

    #[test]
    fn cosmos_profile_differs_from_default_only_in_the_declared_family() -> Result<()> {
        // Guards against silent drift: the cosmos preset is the default filter
        // plus exactly COSMOS_FUSE_PROBE_SYSCALLS, nothing else.
        let default = SeccompProfile::EntelecheiaDefault.to_profile_data();
        let cosmos = SeccompProfile::Cosmos.to_profile_data();
        let names_in = |profile: &SeccompProfileData, action: &str| -> Result<Vec<String>> {
            let mut names = rule(profile, action)?.names.clone();
            names.sort();
            Ok(names)
        };
        for action in ["SCMP_ACT_ALLOW", "SCMP_ACT_ERRNO"] {
            let mut expected = names_in(&default, action)?;
            let actual = names_in(&cosmos, action)?;
            for name in COSMOS_FUSE_PROBE_SYSCALLS.iter().copied() {
                if action == "SCMP_ACT_ALLOW" {
                    if !expected.iter().any(|n| n == name) {
                        expected.push(name.to_string());
                    }
                } else {
                    expected.retain(|n| n != name);
                }
            }
            expected.sort();
            ensure!(
                expected == actual,
                "{action} rule drift: cosmos differs from default outside COSMOS_FUSE_PROBE_SYSCALLS \
                 (expected {expected:?}, got {actual:?})"
            );
        }
        Ok(())
    }

    #[test]
    fn no_profile_lists_one_syscall_in_both_rules() -> Result<()> {
        for profile in [
            SeccompProfile::EntelecheiaDefault.to_profile_data(),
            SeccompProfile::Compile.to_profile_data(),
            SeccompProfile::Cosmos.to_profile_data(),
        ] {
            let allow = rule(&profile, "SCMP_ACT_ALLOW")?;
            let deny = rule(&profile, "SCMP_ACT_ERRNO")?;
            for name in &deny.names {
                ensure!(
                    !allow.names.iter().any(|n| n == name),
                    "{name} is both allowed and denied; rule order would decide it silently"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn cosmos_profile_serializes_to_a_docker_consumable_json() -> Result<()> {
        let json = SeccompProfile::Cosmos.to_json_string()?;
        let parsed: serde_json::Value = serde_json::from_str(&json)?;
        ensure!(
            parsed["defaultAction"] == "SCMP_ACT_ERRNO",
            "deny-by-default action"
        );
        ensure!(
            parsed["architectures"]
                .as_array()
                .is_some_and(|a| a.len() == 4),
            "four architectures"
        );
        let syscalls = parsed["syscalls"].as_array().context("syscalls array")?;
        ensure!(syscalls.len() == 2, "one allow rule and one deny rule");
        Ok(())
    }
}
