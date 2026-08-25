# Security

Cherino enforces defence-in-depth for sandboxed containers through a shared
security-profile layer. The same policy types are consumed by every backend,
so the Docker and Youki runtimes enforce identical restrictions.

## Security profiles

`ContainerSecurity` bundles the per-workload policy:

- `cap_drop` / `cap_add` — Linux capability sets;
- `security_opt` — Docker security options (e.g. `no-new-privileges:true`);
- `egress_policy` — the network egress policy (see below).

Ready-made profiles live in `cherino::security_profile`: `postgres()`,
`scepter()`, `scepter_readonly()`, and `cosmos()`. They encode the platform's
hardened defaults — for example, `scepter()` drops `ALL` capabilities and adds
back only the minimal set its sub-containers need.

With the `docker` feature, `cherino::apply_to_host_config` applies a
`ContainerSecurity` to a Bollard `HostConfig`, translating the policy into
Docker-native settings (including the egress rules).

## seccomp

`SeccompProfile` / `SeccompProfileData` describe syscall-filtering profiles,
and `build_security_opts` renders a profile into `security_opt` entries for
the container config. Seccomp filtering is the first line of defence: it
constrains which syscalls a sandboxed workload may make at all.

## AppArmor

Nested containers need to mount FUSE filesystems, which the default AppArmor
policy blocks. Cherino ships a dedicated profile,
`celestia-cherino-fuse` (`crates/cherino/src/apparmor/celestia-cherino-fuse`),
to be installed on the host once, as root:

```console
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

At runtime, cherino detects which profile is installed
(`installed_profile_name`) and attaches it via `fuse_security_opts`. Hosts
still carrying the legacy `celestia-plana-fuse` profile are accepted with a
deprecation warning.

The escape hatch `CHERINO_APPARMOR_UNCONFINED` (legacy:
`PLANA_APPARMOR_UNCONFINED`) skips AppArmor confinement for hosts where no
profile can be installed — every use emits a `tracing::warn!`.

## Landlock

`LandlockRules` expresses filesystem-access rules enforced through Linux
Landlock, restricting which paths a sandboxed process can touch independently
of container boundaries.

## Egress control

`EgressPolicy` controls outbound network access with three modes
(`EgressMode`): `DenyAll` (default), `AllowAll`, and `Whitelist`. Policies are
built fluently:

```rust
use cherino::EgressPolicy;

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .allow_network("192.0.2.0/24")
    .with_dns_server("192.0.2.53");
```

`EgressPolicy::deny_all()` blocks all egress; `entelecheia_default()` returns
the platform's standard allowlist. On Docker, the policy is realized through
DNS pinning and `extra_hosts` entries so that only whitelisted names resolve
to real addresses.

## Registry whitelisting

`RegistryWhitelist` (with `RegistryEntry`) restricts which image registries
containers may be pulled from. Whitelists can be loaded from a file
(`RegistryWhitelist::load`), parsed from text (`RegistryWhitelist::parse`), or
resolved from a workspace (`resolve_from_workspace`).

## Rootless operation

The `cherino-runtime` (Youki/libcontainer) backend runs containers rootless
and daemonless: no privileged daemon holds container state, and sandboxed
workloads execute as the calling user — shrinking the privileged attack
surface on hosts where it is used.

## Branding and compatibility

`cherino` was extracted from the `plana` workspace. Legacy names are still
read for compatibility, each emitting a `tracing::warn!`:

| Legacy (plana / entelecheia) | New (cherino) |
|------------------------------|---------------|
| AppArmor profile `celestia-plana-fuse` | `celestia-cherino-fuse` |
| `PLANA_APPARMOR_UNCONFINED` env | `CHERINO_APPARMOR_UNCONFINED` env |
| `ENTELECHEIA_RUN_DIR` env | `CHERINO_RUN_DIR` env |
| `/tmp/entelecheia/youki` run dir | `/tmp/cherino/youki` run dir |

The generic overrides (`CONTAINER_RUN_DIR`, `CONTAINER_ROOTFS_URL`,
`CONTAINER_NETWORK`) keep precedence over all branded names.
