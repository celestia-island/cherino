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
    match docker_host_transport(std::env::var("DOCKER_HOST").ok().as_deref()) {
        Ok(Some(DockerHostTransport::Http(host))) => connect_http(&host),
        Ok(Some(DockerHostTransport::Unix(path))) => connect_socket(&path),
        Ok(None) => Docker::connect_with_local_defaults()
            .map_err(|e| ContainerError::Connection(e.to_string())),
        Err(e) => Err(e),
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
                text.contains("unix://") && text.contains("tcp://"),
                "the error must name the recognised schemes: {text}"
            );
        }
    }
}
