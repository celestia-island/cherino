use bollard::Docker;

use crate::errors::{ContainerError, ContainerResult};

/// Transport named by a `DOCKER_HOST` value, when it is one this module honours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DockerHostTransport {
    /// `tcp://host:port` or `http://host:port` — a TCP daemon or socket proxy.
    /// Plaintext: acceptable only across an isolated docker bridge, never over an
    /// untrusted network.
    Http(String),
    /// `unix:///path/to.sock` — a local unix socket.
    Unix(String),
}

/// Classify a `DOCKER_HOST` value.
///
/// bollard's `connect_with_local_defaults()` honours `DOCKER_HOST` only when it
/// starts with `unix://`; a `tcp://` value is silently ignored and the client
/// connects to `/var/run/docker.sock` instead. That silent downgrade is how a
/// scepter deployed behind a docker-socket-proxy ends up in local-execution mode
/// while a raw socket is also mounted — an isolation bypass nobody can see.
///
/// So: unset or blank ⇒ `Ok(None)` (the default-socket path, byte-identical to
/// before); a recognised scheme ⇒ that transport; **anything else ⇒ an error
/// naming the recognised schemes**, never a silent fallback. Schemes are matched
/// case-insensitively (the Go client accepts `TCP://`), and a scheme with no
/// address after it is an error too.
pub(crate) fn docker_host_transport(
    value: Option<&str>,
) -> ContainerResult<Option<DockerHostTransport>> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let host = raw.trim();
    if host.is_empty() {
        return Ok(None);
    }
    let lower = host.to_ascii_lowercase();
    // Rebuild the value with a LOWERCASE scheme and the caller's original
    // remainder: bollard strips the scheme case-sensitively (`replacen("tcp://")`),
    // so handing it `TCP://…` would classify here and then fail to connect with the
    // scheme still embedded. The remainder keeps its case — unix socket paths are
    // case-sensitive.
    for scheme in ["tcp://", "http://"] {
        if lower.starts_with(scheme) {
            if host.len() <= scheme.len() {
                return Err(unsupported_docker_host(raw));
            }
            return Ok(Some(DockerHostTransport::Http(format!(
                "{scheme}{}",
                &host[scheme.len()..]
            ))));
        }
    }
    if lower.starts_with("unix://") {
        if host.len() <= "unix://".len() {
            return Err(unsupported_docker_host(raw));
        }
        return Ok(Some(DockerHostTransport::Unix(format!(
            "unix://{}",
            &host["unix://".len()..]
        ))));
    }
    Err(unsupported_docker_host(raw))
}

fn unsupported_docker_host(raw: &str) -> ContainerError {
    ContainerError::Connection(format!(
        "DOCKER_HOST={raw:?} is set but is not a transport cherino can honour. \
         Use unix://<path>, tcp://<host>:<port> or http://<host>:<port>; tcp/http are \
         PLAINTEXT and only acceptable across an isolated docker bridge. Leave \
         DOCKER_HOST unset to keep the local socket default."
    ))
}

pub fn connect_local() -> ContainerResult<Docker> {
    dispatch_docker_host(std::env::var("DOCKER_HOST").ok().as_deref())
}

/// The dispatch behind [`connect_local`], with the value injected so every arm can
/// be pinned without touching the process environment.
///
/// The arms are distinguishable without a daemon because bollard is asymmetric:
/// `connect_with_http` performs no eager validation (a lazy client is `Ok`), while
/// `connect_with_socket` eagerly `exists()`-checks the path (a missing socket is
/// an `Err`). A mutation that swaps the arms, drops the error arm, or ignores the
/// value therefore changes an observable result instead of staying green.
pub(crate) fn dispatch_docker_host(value: Option<&str>) -> ContainerResult<Docker> {
    match docker_host_transport(value)? {
        Some(DockerHostTransport::Http(host)) => connect_http(&host),
        Some(DockerHostTransport::Unix(path)) => connect_socket(&path),
        None => Docker::connect_with_local_defaults()
            .map_err(|e| ContainerError::Connection(e.to_string())),
    }
}

pub fn connect_socket(socket_path: &str) -> ContainerResult<Docker> {
    Docker::connect_with_socket(socket_path, 120, bollard::API_DEFAULT_VERSION)
        .map_err(|e| ContainerError::Connection(e.to_string()))
}

pub fn connect_http(host: &str) -> ContainerResult<Docker> {
    Docker::connect_with_http(host, 120, bollard::API_DEFAULT_VERSION)
        .map_err(|e| ContainerError::Connection(e.to_string()))
}

#[cfg(windows)]
pub fn connect_named_pipe(pipe_name: &str) -> ContainerResult<Docker> {
    Docker::connect_with_named_pipe(pipe_name, 30, bollard::API_DEFAULT_VERSION)
        .map_err(|e| ContainerError::Connection(e.to_string()))
}

pub fn connect_auto() -> ContainerResult<Docker> {
    #[cfg(windows)]
    {
        connect_named_pipe("//./pipe/docker_engine")
    }
    #[cfg(not(windows))]
    {
        connect_local()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(value: &str) -> ContainerResult<Option<DockerHostTransport>> {
        docker_host_transport(Some(value))
    }

    #[test]
    fn tcp_and_http_are_honoured_case_insensitively() {
        for value in [
            "tcp://cherino-socket-proxy:2375",
            "TCP://cherino-socket-proxy:2375",
            "http://127.0.0.1:2375",
            "HTTP://127.0.0.1:2375",
            "tcp://[::1]:2375",
            "  tcp://proxy:2375  ",
        ] {
            assert!(
                matches!(classify(value), Ok(Some(DockerHostTransport::Http(_)))),
                "expected an HTTP transport for {value:?}"
            );
        }
    }

    /// The classifier must hand bollard a LOWERCASE scheme: bollard strips the
    /// scheme case-sensitively, so `TCP://…` would classify and then fail at request
    /// time. The remainder keeps its original case (socket paths are case-sensitive).
    #[test]
    fn the_transport_carries_a_lowercase_scheme() {
        match classify("TCP://proxy:2375") {
            Ok(Some(DockerHostTransport::Http(v))) => assert_eq!(v, "tcp://proxy:2375"),
            other => panic!("expected Http(tcp://proxy:2375), got {other:?}"),
        }
        match classify("HTTP://proxy:2375") {
            Ok(Some(DockerHostTransport::Http(v))) => assert_eq!(v, "http://proxy:2375"),
            other => panic!("expected Http(http://proxy:2375), got {other:?}"),
        }
        match classify("UNIX:///Run/Docker.sock") {
            Ok(Some(DockerHostTransport::Unix(v))) => assert_eq!(v, "unix:///Run/Docker.sock"),
            other => panic!("expected Unix with the path case preserved, got {other:?}"),
        }
    }

    /// The dispatch arms, pinned by bollard's eager/lazy asymmetry: a TCP client is
    /// lazy (Ok even for a host that cannot resolve), a unix client eagerly checks
    /// the path (Err for a missing socket), and an unrecognised value propagates the
    /// classifier's error instead of falling back to the local socket.
    #[test]
    fn every_dispatch_arm_is_observable_without_a_daemon() {
        assert!(
            dispatch_docker_host(Some("tcp://nowhere.invalid:1")).is_ok(),
            "a TCP transport must be handed to a lazy client, not rejected here"
        );
        assert!(
            dispatch_docker_host(Some("unix:///nonexistent-cherino-test.sock")).is_err(),
            "a unix transport must reach connect_with_socket, which eagerly checks the path"
        );
        let err = dispatch_docker_host(Some("fd://")).expect_err("unrecognised must propagate");
        assert!(
            format!("{err}").contains("tcp://"),
            "the propagated error must name the recognised schemes: {err}"
        );
    }

    /// The environment read itself, and the `None` arm behind it.
    ///
    /// One combined test on purpose: both halves mutate `DOCKER_HOST`, so splitting
    /// them would let two test threads race on the same variable. Mirrors the
    /// precedent in `lifecycle.rs` (`network_default_tests`): save, mutate under a
    /// SAFETY comment, assert, restore.
    ///
    /// Portability: the first half is cross-platform (`connect_with_socket` does its
    /// own `exists()` check). The second half is `#[cfg(unix)]` only — on Windows
    /// `connect_with_local_defaults()` routes to bollard's **named-pipe** defaults,
    /// which build a client with no eager validation and therefore return `Ok`.
    ///
    /// Known limitation, recorded rather than engineered around: if an assertion
    /// panics, the restore is skipped and `DOCKER_HOST` stays mutated for the rest of
    /// the process. That can only cascade failures (never fake a pass), and a guard
    /// type would not make the failure any clearer.
    #[test]
    fn connect_local_reads_the_environment() {
        let ambient = std::env::var("DOCKER_HOST").ok();
        // SAFETY: test-only env mutation. The crate's only other env-mutating test
        // is in `lifecycle` and touches a different variable; cargo may still run
        // tests in parallel, so this test owns DOCKER_HOST for its duration.
        unsafe {
            // An unsupported value must surface as an error, which also proves the
            // environment was read at all: a `dispatch_docker_host(None)` mutation
            // (or a typo'd variable name) would reach the local-defaults path and
            // return Ok on a host that has a socket.
            std::env::set_var("DOCKER_HOST", "fd://");
            let err = connect_local().expect_err("DOCKER_HOST=fd:// must be rejected");
            assert!(
                format!("{err}").contains("fd://"),
                "the error must quote the rejected value: {err}"
            );

            // The `None` arm must reach bollard's local defaults, which themselves
            // honour a unix:// DOCKER_HOST and eagerly check the path — so a
            // `None => connect_http(..)` mutation (lazy, always Ok) is observable
            // here as the absence of an error. Unix only: Windows local defaults use
            // named pipes, which validate nothing eagerly.
            #[cfg(unix)]
            {
                std::env::set_var("DOCKER_HOST", "unix:///nonexistent-cherino-local.sock");
                assert!(
                    dispatch_docker_host(None).is_err(),
                    "None must reach connect_with_local_defaults, which honours unix:// and checks the path"
                );
            }
        }
        match ambient {
            Some(value) => unsafe { std::env::set_var("DOCKER_HOST", value) },
            None => unsafe { std::env::remove_var("DOCKER_HOST") },
        }
    }

    #[test]
    fn whitespace_and_mixed_case_hosts_are_normalised() {
        match classify("  tcp://ProXy:2375  ") {
            Ok(Some(DockerHostTransport::Http(v))) => assert_eq!(v, "tcp://ProXy:2375"),
            other => panic!("expected a trimmed tcp transport, got {other:?}"),
        }
    }

    #[test]
    fn unix_sockets_are_honoured_case_insensitively() {
        for value in ["unix:///run/docker.sock", "UNIX:///run/docker.sock"] {
            assert!(
                matches!(classify(value), Ok(Some(DockerHostTransport::Unix(_)))),
                "expected a unix transport for {value:?}"
            );
        }
    }

    #[test]
    fn unset_or_blank_keeps_the_default_socket_path() {
        assert!(matches!(docker_host_transport(None), Ok(None)));
        assert!(matches!(classify(""), Ok(None)));
        assert!(matches!(classify("   "), Ok(None)));
    }

    /// A set-but-unrecognised value must FAIL LOUD instead of silently connecting
    /// to the local socket — that silence is the isolation bypass.
    #[test]
    fn unrecognised_values_fail_loud() {
        for value in [
            "fd://",
            "ssh://user@host",
            "npipe:////./pipe/docker_engine",
            "https://example.invalid:2376",
            "/var/run/docker.sock",
            "unix://",
            "tcp://",
            "cherino-socket-proxy:2375",
        ] {
            let err = classify(value).expect_err(&format!("{value:?} must be rejected"));
            let text = format!("{err}");
            assert!(
                text.contains("unix://")
                    && text.contains("tcp://")
                    && text.contains("http://")
                    && text.contains("PLAINTEXT")
                    && text.contains("unset"),
                "the error must name the recognised schemes: {text}"
            );
        }
    }
}
