//! Named AppArmor profile for FUSE-capable containers (scepter / cosmos).
//!
//! Docker's default AppArmor profile (`docker-default`) denies FUSE mounts,
//! which `fuse-overlayfs` needs to isolate cosmos sub-container root
//! filesystems. Instead of disabling AppArmor entirely with
//! `apparmor=unconfined`, we apply the named [`FUSE_PROFILE_NAME`] profile,
//! which mirrors the docker-default baseline and additionally allows FUSE
//! mounts plus the pivot_root/overlay operations nested container
//! orchestration requires.
//!
//! ## Loading the profile (host, root)
//!
//! [`FUSE_PROFILE`] must be installed and loaded on the Docker host before
//! containers are created with it:
//!
//! ```text
//! # install
//! install -m 0644 celestia-cherino-fuse /etc/apparmor.d/celestia-cherino-fuse
//! # load into the running kernel
//! apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
//! ```
//!
//! If the profile is not loaded, Docker rejects container creation with a
//! clear error — this is the intended fail-closed path: the profile name is
//! set unconditionally (unless the dev escape hatch is enabled), so a missing
//! profile cannot silently fall back to `unconfined`.
//!
//! ## Legacy `celestia-plana-fuse` compatibility
//!
//! This crate was extracted from the plana workspace, where the profile was
//! named `celestia-plana-fuse`. Hosts that still carry the legacy profile
//! file are detected by [`installed_profile_name`], which accepts the legacy
//! name with a deprecation warning so existing deployments keep working
//! while the host is migrated to the new name.
//!
//! # DEV-ONLY escape hatch
//!
//! The `CHERINO_APPARMOR_UNCONFINED` environment variable (see
//! [`is_apparmor_unconfined`]; the legacy `PLANA_APPARMOR_UNCONFINED` name is
//! still honored) falls back to `apparmor=unconfined` and MUST NEVER be set
//! in production.

use std::path::Path;

use tracing::warn;

/// Name of the AppArmor profile that permits FUSE mounts (fuse-overlayfs).
pub const FUSE_PROFILE_NAME: &str = "celestia-cherino-fuse";

/// Deprecated plana-era profile name, still accepted when it is the only one
/// installed on the host (see [`installed_profile_name`]).
pub const LEGACY_FUSE_PROFILE_NAME: &str = "celestia-plana-fuse";

/// Directory the host installs AppArmor profiles into.
const APPARMOR_PROFILE_DIR: &str = "/etc/apparmor.d";

/// Dev-mode escape hatch: set `CHERINO_APPARMOR_UNCONFINED=1` to fall back to
/// `apparmor=unconfined`.
pub const UNCONFINED_ENV: &str = "CHERINO_APPARMOR_UNCONFINED";

/// Deprecated plana-era name of the escape hatch env var, still read for
/// compatibility (see [`is_apparmor_unconfined`]).
pub const LEGACY_UNCONFINED_ENV: &str = "PLANA_APPARMOR_UNCONFINED";

/// The AppArmor profile to load on the host via `apparmor_parser -r`.
pub const FUSE_PROFILE: &str = include_str!("apparmor/celestia-cherino-fuse");

/// Returns the profile name a container should reference right now.
///
/// The new [`FUSE_PROFILE_NAME`] is canonical: it is used whenever its
/// profile file is installed on the host (or when neither name is installed —
/// the fail-closed default, so Docker surfaces a clear "profile not loaded"
/// error instead of silently degrading). If only the legacy
/// [`LEGACY_FUSE_PROFILE_NAME`] file is installed, the legacy name is
/// accepted with a deprecation warning so pre-cherino hosts keep working.
pub fn installed_profile_name() -> &'static str {
    let dir = Path::new(APPARMOR_PROFILE_DIR);
    pick_profile_name(
        dir.join(FUSE_PROFILE_NAME).exists(),
        dir.join(LEGACY_FUSE_PROFILE_NAME).exists(),
    )
}

/// Pure decision for the profile-name compat pick, kept separate for tests.
fn pick_profile_name(new_installed: bool, legacy_installed: bool) -> &'static str {
    if new_installed {
        return FUSE_PROFILE_NAME;
    }
    if legacy_installed {
        warn!(
            legacy_profile = LEGACY_FUSE_PROFILE_NAME,
            new_profile = FUSE_PROFILE_NAME,
            "only the legacy plana-era AppArmor profile is installed; \
             accepting the legacy name (deprecated) — install {} to migrate",
            FUSE_PROFILE_NAME
        );
        return LEGACY_FUSE_PROFILE_NAME;
    }
    FUSE_PROFILE_NAME
}

/// Returns true when the AppArmor-unconfined escape hatch is enabled via
/// [`UNCONFINED_ENV`] (or the deprecated [`LEGACY_UNCONFINED_ENV`]).
///
/// # DEV-ONLY escape hatch — never set in production
///
/// `CHERINO_APPARMOR_UNCONFINED=true` (or `1`) makes [`fuse_security_opts`]
/// emit `apparmor=unconfined`, disabling the named FUSE AppArmor profile.
/// This mirrors the `DISABLE_SECCOMP` escape hatch in [`crate::seccomp`] and
/// exists only to unblock local development on hosts where the named profile
/// has not yet been installed/loaded. It MUST NEVER be set in production.
pub fn is_apparmor_unconfined() -> bool {
    let value = std::env::var(UNCONFINED_ENV).ok().or_else(|| {
        std::env::var(LEGACY_UNCONFINED_ENV)
            .ok()
            .filter(|v| !v.is_empty())
            .inspect(|_| {
                warn!(
                    env = LEGACY_UNCONFINED_ENV,
                    new_env = UNCONFINED_ENV,
                    "deprecated plana-era env var used; rename it to {}",
                    UNCONFINED_ENV
                );
            })
    });

    let unconfined = unconfined_from_value(value.as_deref());

    if unconfined {
        warn!(
            env = UNCONFINED_ENV,
            "escape hatch is set: the named AppArmor profile is DISABLED \
             and apparmor=unconfined is used instead. This is a DEV-ONLY escape hatch \
             and must never be enabled in production."
        );
    }

    unconfined
}

fn unconfined_from_value(value: Option<&str>) -> bool {
    matches!(value, Some(v) if v.eq_ignore_ascii_case("true") || v == "1")
}

/// Security opts for scepter/cosmos: the named FUSE AppArmor profile (or the
/// `unconfined` fallback) plus `no-new-privileges`.
///
/// The profile name reflects what is actually installed on the host (see
/// [`installed_profile_name`]): the cherino name by default, the legacy
/// plana-era name when that is the only profile present.
pub fn fuse_security_opts() -> Vec<String> {
    fuse_security_opts_with_unconfined(is_apparmor_unconfined(), installed_profile_name())
}

fn fuse_security_opts_with_unconfined(unconfined: bool, profile_name: &str) -> Vec<String> {
    let mut opts = vec!["no-new-privileges:true".to_string()];

    if unconfined {
        opts.push("apparmor=unconfined".to_string());
    } else {
        opts.push(format!("apparmor={}", profile_name));
    }

    opts
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Result, ensure};

    #[test]
    fn named_profile_security_opts() -> Result<()> {
        let opts = fuse_security_opts_with_unconfined(false, FUSE_PROFILE_NAME);
        ensure!(
            opts.contains(&"no-new-privileges:true".to_string()),
            "must keep no-new-privileges: {:?}",
            opts
        );
        ensure!(
            opts.contains(&format!("apparmor={}", FUSE_PROFILE_NAME)),
            "must use the named profile by default: {:?}",
            opts
        );
        ensure!(
            !opts.iter().any(|o| o == "apparmor=unconfined"),
            "must not use apparmor=unconfined by default: {:?}",
            opts
        );
        Ok(())
    }

    #[test]
    fn unconfined_fallback_security_opts() -> Result<()> {
        let opts = fuse_security_opts_with_unconfined(true, FUSE_PROFILE_NAME);
        ensure!(
            opts.contains(&"apparmor=unconfined".to_string()),
            "escape hatch must fall back to apparmor=unconfined: {:?}",
            opts
        );
        ensure!(
            opts.contains(&"no-new-privileges:true".to_string()),
            "no-new-privileges must remain even when unconfined: {:?}",
            opts
        );
        Ok(())
    }

    #[test]
    fn unconfined_parses_only_truthy_values() -> Result<()> {
        assert!(unconfined_from_value(Some("true")));
        assert!(unconfined_from_value(Some("TRUE")));
        assert!(unconfined_from_value(Some("1")));
        assert!(!unconfined_from_value(Some("false")));
        assert!(!unconfined_from_value(Some("0")));
        assert!(!unconfined_from_value(Some("")));
        assert!(!unconfined_from_value(None));
        Ok(())
    }

    #[test]
    fn profile_content_permits_fuse_and_pivot_root() -> Result<()> {
        ensure!(
            FUSE_PROFILE.contains("mount fstype=fuse.*"),
            "profile must allow FUSE mounts"
        );
        ensure!(
            FUSE_PROFILE.contains("pivot_root"),
            "profile must keep docker-default pivot_root allowance"
        );
        ensure!(
            FUSE_PROFILE.contains(&format!("profile {}", FUSE_PROFILE_NAME)),
            "profile block must be named after FUSE_PROFILE_NAME"
        );
        ensure!(
            FUSE_PROFILE.contains("#include <tunables/global>"),
            "profile must include tunables/global"
        );
        ensure!(
            !FUSE_PROFILE.contains(LEGACY_FUSE_PROFILE_NAME),
            "bundled profile must not reference the legacy plana-era name"
        );
        Ok(())
    }

    #[test]
    fn profile_name_prefers_new_then_legacy_then_fail_closed() -> Result<()> {
        ensure!(
            pick_profile_name(true, true) == FUSE_PROFILE_NAME,
            "new profile installed must win even if legacy is also present"
        );
        ensure!(
            pick_profile_name(true, false) == FUSE_PROFILE_NAME,
            "new profile installed must be used"
        );
        ensure!(
            pick_profile_name(false, true) == LEGACY_FUSE_PROFILE_NAME,
            "legacy-only hosts must keep working with the legacy name"
        );
        ensure!(
            pick_profile_name(false, false) == FUSE_PROFILE_NAME,
            "no profile installed must fail closed on the new name"
        );
        Ok(())
    }
}
