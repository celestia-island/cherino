//! Live seccomp boundary test for the cosmos preset (R2).
//!
//! This is the negative/positive control for the profile change: it runs the
//! same syscall probe (`tests/fixtures/syscall_probe.c`) in three containers
//! that differ only in their `--security-opt seccomp=` argument, and asserts
//! that the filter — not capabilities, not AppArmor, not the harness — is what
//! moves each syscall.
//!
//! | run | filter | mount/unshare/ptrace/bpf |
//! | --- | --- | --- |
//! | `unconfined` | none (the live sandbox before R2) | all allowed |
//! | `entelecheia_default` | the workload default | all refused ⇒ the reason the gap existed |
//! | `cosmos` | the new preset | mount/umount2/unshare allowed; ptrace/bpf/process_vm_readv still refused |
//!
//! The matrix runs with `apparmor=unconfined` so that every observed EPERM can
//! only come from seccomp: the AppArmor half is a different layer and would
//! otherwise mask the result (it permits `fstype=fuse.*`/`overlay`, not the
//! `tmpfs` the probe mounts). The FUSE test is the other half — set
//! `CHERINO_SECCOMP_TEST_APPARMOR=celestia-cherino-fuse` on a host that has the
//! named profile loaded and it runs the production pairing end to end.
//!
//! Run with:
//!
//! ```console
//! CHERINO_SECCOMP_TEST_IMAGE=<image with gcc> \
//!   cargo test -p cherino --features docker-tests --test seccomp_boundary -- --ignored --nocapture
//! ```
//!
//! The optional FUSE run additionally needs an image containing
//! `fuse-overlayfs` and a host with `/dev/fuse`:
//!
//! ```console
//! CHERINO_SECCOMP_TEST_FUSE_IMAGE=<image with fuse-overlayfs> ...
//! ```
#![cfg(feature = "docker-tests")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use cherino::seccomp::SeccompProfile;

const EPERM: i32 = 1;
const EINVAL: i32 = 22;

/// Where `syscall_probe.c` lives.
///
/// `CARGO_MANIFEST_DIR` is baked in at compile time, and this workspace compiles
/// in a container whose checkout is not the one the test later runs from (the
/// container lane has no Docker socket). The explicit override wins, then the
/// compiled-in path, then the path relative to the checkout root the runner was
/// started from.
fn fixture_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CHERINO_SECCOMP_TEST_FIXTURES") {
        return PathBuf::from(dir);
    }
    let compiled = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");
    if compiled.join("syscall_probe.c").exists() {
        return compiled;
    }
    let relative = Path::new("crates/cherino/tests/fixtures").to_path_buf();
    assert!(
        relative.join("syscall_probe.c").exists(),
        "syscall_probe.c not found: set CHERINO_SECCOMP_TEST_FIXTURES to the fixtures directory"
    );
    relative
}

fn image() -> Option<String> {
    std::env::var("CHERINO_SECCOMP_TEST_IMAGE").ok()
}

/// Which filter a probe container runs under.
enum Filter {
    /// No filter at all: the live sandbox before R2.
    Unconfined,
    /// One of the crate's profiles, written to a file (Docker takes a path).
    Profile(SeccompProfile),
}

/// Runs the probe in one container and returns `name -> errno` (0 = completed).
///
/// The profile travels as a file because Docker's `--security-opt seccomp=`
/// takes a path, not the document (the daemon accepts inline JSON through the
/// API, the CLI does not).
fn run_probe(filter: Filter, image: &str) -> BTreeMap<String, i32> {
    let mut command = Command::new("docker");
    command.args([
        "run",
        "--rm",
        "--network",
        "none",
        "--cap-add",
        "SYS_ADMIN",
        "--security-opt",
        "apparmor=unconfined",
    ]);
    let profile_dir = tempfile::tempdir().expect("temp dir for the seccomp profile");
    let label = match filter {
        Filter::Unconfined => {
            command.args(["--security-opt", "seccomp=unconfined"]);
            "unconfined".to_string()
        }
        Filter::Profile(profile) => {
            let path = profile_dir.path().join("seccomp.json");
            std::fs::write(&path, profile.to_json_string().expect("profile serializes"))
                .expect("write seccomp profile");
            // The Docker CLI opens the profile itself and sends the document, so
            // the path is a *host* path: a path inside the container never works.
            command.arg("--security-opt");
            command.arg(format!("seccomp={}", path.display()));
            format!("{profile:?}")
        }
    };
    let output = command
        .args(["-v"])
        .arg(format!("{}:/fixture:ro", fixture_dir().display()))
        .arg(image)
        .args([
            "sh",
            "-c",
            "gcc -O1 -Wall -o /tmp/probe /fixture/syscall_probe.c && /tmp/probe",
        ])
        .output()
        .unwrap_or_else(|e| panic!("docker run failed to spawn: {e}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "probe container failed (filter={label}):\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    parse_probe(&stdout)
}

/// The `security_opt` keys only: the seccomp body is a multi-kilobyte JSON
/// document and must not be dumped into test output.
fn requested_keys(opts: &[String]) -> String {
    opts.iter()
        .map(|o| o.split(['=', ':']).next().unwrap_or(o).to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_probe(stdout: &str) -> BTreeMap<String, i32> {
    let mut seen = BTreeMap::new();
    for line in stdout.lines() {
        let Some((name, value)) = line.trim().split_once('=') else {
            continue;
        };
        if let Ok(errno) = value.parse::<i32>() {
            seen.insert(name.to_string(), errno);
        }
    }
    for required in [
        "getpid",
        "mount",
        "umount2",
        "unshare",
        "ptrace",
        "bpf",
        "process_vm_readv",
    ] {
        assert!(
            seen.contains_key(required),
            "probe did not report `{required}`; got {seen:?}\nstdout:\n{stdout}"
        );
    }
    seen
}

#[test]
#[ignore = "requires docker + CHERINO_SECCOMP_TEST_IMAGE (see module docs)"]
fn cosmos_seccomp_boundary_matrix() {
    let Some(image) = image() else {
        panic!("set CHERINO_SECCOMP_TEST_IMAGE to an image that has `gcc` and `sh`");
    };

    // Baseline: no filter at all — this is the live sandbox before R2, and it
    // proves the harness, the capabilities and AppArmor alone allow everything.
    let unrestricted = run_probe(Filter::Unconfined, &image);
    assert_eq!(unrestricted["getpid"], 0, "harness sanity");
    assert_eq!(
        unrestricted["mount"], 0,
        "the baseline must be able to mount, or the comparison proves nothing: {unrestricted:?}"
    );
    assert_eq!(unrestricted["unshare"], 0, "baseline unshare");
    assert_eq!(unrestricted["ptrace"], 0, "baseline ptrace");
    assert_eq!(
        unrestricted["bpf"], EINVAL,
        "baseline bpf reaches the kernel and is rejected on its arguments"
    );

    // The workload default — the filter that was too strict for cosmos. This is
    // the documented reason the preset shipped AppArmor-only.
    let default = run_probe(Filter::Profile(SeccompProfile::EntelecheiaDefault), &image);
    assert_eq!(
        default["mount"], EPERM,
        "the default profile blocks mount (why the cosmos preset had no filter)"
    );
    assert_eq!(
        default["unshare"], EPERM,
        "the default profile blocks unshare"
    );
    assert_eq!(
        default["ptrace"], EPERM,
        "the default profile blocks ptrace"
    );
    assert_eq!(default["bpf"], EPERM, "the default profile blocks bpf");

    // The new cosmos preset: the FUSE/OCI family becomes reachable, the kernel
    // attack surface does not.
    let cosmos = run_probe(Filter::Profile(SeccompProfile::Cosmos), &image);
    // Positive control: a normal cosmos workload (the rootfs probe + the inner
    // OCI runtime) can mount and create a namespace.
    assert_eq!(cosmos["mount"], 0, "cosmos must allow mount: {cosmos:?}");
    assert_eq!(
        cosmos["umount2"], 0,
        "cosmos must allow umount2: {cosmos:?}"
    );
    assert_eq!(
        cosmos["unshare"], 0,
        "cosmos must allow unshare: {cosmos:?}"
    );
    // Negative control: the escape surface is still refused.
    assert_eq!(cosmos["ptrace"], EPERM, "cosmos must still refuse ptrace");
    assert_eq!(cosmos["bpf"], EPERM, "cosmos must still refuse bpf");
    assert_eq!(
        cosmos["process_vm_readv"], EPERM,
        "cosmos must still refuse process_vm_readv"
    );

    println!("unconfined: {unrestricted:?}");
    println!("default:    {default:?}");
    println!("cosmos:     {cosmos:?}");
}

/// The real probe binary, in the real image that runs it: `fuse-overlayfs`
/// mounting a merged rootfs under the cosmos filter and the FUSE device.
///
/// `CHERINO_SECCOMP_TEST_APPARMOR` selects the LSM half: `unconfined` (default,
/// works anywhere) or `celestia-cherino-fuse` (the production profile — the
/// complete pairing, needs the profile loaded on the host).
#[test]
#[ignore = "requires docker, /dev/fuse, CHERINO_SECCOMP_TEST_FUSE_IMAGE (an image with fuse-overlayfs)"]
fn cosmos_seccomp_allows_the_fuse_rootfs_probe() {
    let Some(image) = std::env::var("CHERINO_SECCOMP_TEST_FUSE_IMAGE").ok() else {
        panic!("set CHERINO_SECCOMP_TEST_FUSE_IMAGE to an image that has `fuse-overlayfs`");
    };
    if !Path::new("/dev/fuse").exists() {
        panic!("this host has no /dev/fuse; the FUSE probe cannot run here");
    }
    let profile_dir = tempfile::tempdir().expect("temp dir for the seccomp profile");
    std::fs::write(
        profile_dir.path().join("seccomp.json"),
        SeccompProfile::Cosmos
            .to_json_string()
            .expect("profile serializes"),
    )
    .expect("write seccomp profile");
    let script = r#"
set -e
mkdir -p /tmp/p/upper /tmp/p/work /tmp/p/merged /tmp/p/lower
echo hello > /tmp/p/lower/seed.txt
fuse-overlayfs -o lowerdir=/tmp/p/lower,upperdir=/tmp/p/upper,workdir=/tmp/p/work /tmp/p/merged
cat /tmp/p/merged/seed.txt
mountpoint -q /tmp/p/merged && echo MERGED_IS_A_MOUNTPOINT
fusermount3 -u /tmp/p/merged || umount /tmp/p/merged
echo PROBE_OK
"#;
    let output = Command::new("docker")
        .args([
            "run",
            "--rm",
            "--network",
            "none",
            "--cap-add",
            "SYS_ADMIN",
            "--device",
            "/dev/fuse",
            "--security-opt",
        ])
        .arg(format!(
            "apparmor={}",
            std::env::var("CHERINO_SECCOMP_TEST_APPARMOR")
                .unwrap_or_else(|_| "unconfined".to_string())
        ))
        .arg("--security-opt")
        .arg(format!(
            "seccomp={}",
            profile_dir.path().join("seccomp.json").display()
        ))
        .arg(&image)
        .args(["sh", "-c", script])
        .output()
        .unwrap_or_else(|e| panic!("docker run failed to spawn: {e}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "fuse-overlayfs probe failed under the cosmos filter:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("hello") && stdout.contains("PROBE_OK"),
        "the merged rootfs must serve its lower layer:\nstdout:\n{stdout}"
    );
    println!("{stdout}");
}

/// The wiring, not just the comparison: a container created through the real
/// `ContainerManager` with the real `cosmos()` preset must pass the startup
/// self-check and come back carrying both halves of the boundary.
///
/// This is the only test that exercises the post-start inspect path end to end;
/// the unit tests cover the comparison itself, and drift cannot be induced on a
/// live daemon (which is why the failure branch is exercised by those units).
#[tokio::test]
#[ignore = "requires docker + CHERINO_SECCOMP_TEST_IMAGE"]
async fn container_create_keeps_the_cosmos_profile_and_passes_the_self_check() {
    use cherino::security_profile;

    let Some(image) = image() else {
        panic!("set CHERINO_SECCOMP_TEST_IMAGE to an image that has `sh`");
    };
    let manager = cherino::ContainerManager::new().expect("docker connection");

    let preset = security_profile::cosmos();
    let mut params = cherino::types::ContainerCreateParams::simple(
        format!("cherino-r2-seccomp-{}", std::process::id()),
        image,
    );
    params.network = Some("bridge".to_string());
    params.cap_drop = preset.cap_drop.clone();
    params.cap_add = preset.cap_add.clone();
    params.security_opt = preset.security_opt.clone();
    params.read_only_rootfs = Some(false);
    params.egress_policy = None;

    // `create` runs the self-check: a profile the runtime did not apply would
    // come back as `SecurityProfileDrift` here, not as a healthy container.
    let info = manager
        .create(&params)
        .await
        .expect("create must pass the startup security self-check");

    // `create` succeeding is the wiring proof: it runs the same comparison on
    // the recorded options and fails the create on drift. Read them back anyway
    // so the evidence is in the test output, not only in the code path.
    let output = Command::new("docker")
        .args([
            "inspect",
            "--format",
            "{{json .HostConfig.SecurityOpt}}",
            &info.id,
        ])
        .output()
        .expect("docker inspect");
    assert!(
        output.status.success(),
        "docker inspect failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let recorded: Vec<String> = serde_json::from_slice(&output.stdout).expect("security_opt json");
    let requested = params.security_opt.clone().unwrap_or_default();
    security_profile::verify_effective_security_opts(&requested, &recorded)
        .expect("the recorded security_opt must match the requested one");
    assert!(
        recorded.iter().any(|o| o.starts_with("seccomp=")),
        "seccomp must be recorded on the container: {recorded:?}"
    );
    assert!(
        recorded
            .iter()
            .any(|o| o.starts_with("apparmor=") && o != "apparmor=unconfined"),
        "the named AppArmor profile must be recorded: {recorded:?}"
    );
    println!("recorded security_opt keys: {}", requested_keys(&recorded));

    manager.remove(&info.id, true).await.expect("cleanup");
}
