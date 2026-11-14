use bollard::Docker;

use crate::errors::{ContainerError, ContainerResult};

/// Transport named by a `DOCKER_HOST` value, when it is one this module can honour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerHostTransport {
    /// `tcp://host:port` or `http://host:port` — a TCP daemon or socket proxy.
    Http(String),
    /// `unix:///path/to.sock` — a local unix socket.
    Unix(String),
}

/// Classify a `DOCKER_HOST` value.
///
/// bollard's `connect_with_local_defaults()` only honours `DOCKER_HOST` when it
/// starts with `unix://`; a `tcp://` value is silently ignored and the client
/// falls back to `/var/run/docker.sock`. That is exactly how a scepter deployed
/// behind a docker-socket-proxy ends up in local-execution mode with a
/// "Socket not found: /var/run/docker.sock" warning. Anything this function does
/// not classify (npipe, ssh, empty) keeps the previous behaviour.
pub fn docker_host_transport(value: Option<&str>) -> Option<DockerHostTransport> {
    let host = value?.trim();
    if host.is_empty() {
        return None;
    }
    if host.starts_with("tcp://") || host.starts_with("http://") {
        Some(DockerHostTransport::Http(host.to_string()))
    } else if host.starts_with("unix://") {
        Some(DockerHostTransport::Unix(host.to_string()))
    } else {
        // `https://` is not supported by bollard's plain http connector, and
        // `npipe://`/`ssh://` have their own helpers; leaving them `None` keeps
        // the pre-existing local-defaults behaviour for those deployments.
        None
    }
}

pub fn connect_local() -> ContainerResult<Docker> {
    match docker_host_transport(std::env::var("DOCKER_HOST").ok().as_deref()) {
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

    #[test]
    fn docker_host_tcp_is_honoured() {
        assert_eq!(
            docker_host_transport(Some("tcp://cherino-socket-proxy:2375")),
            Some(DockerHostTransport::Http(
                "tcp://cherino-socket-proxy:2375".to_string()
            ))
        );
        assert_eq!(
            docker_host_transport(Some("http://127.0.0.1:2375")),
            Some(DockerHostTransport::Http(
                "http://127.0.0.1:2375".to_string()
            ))
        );
    }

    #[test]
    fn docker_host_unix_still_wins_for_sockets() {
        assert_eq!(
            docker_host_transport(Some("unix:///run/docker.sock")),
            Some(DockerHostTransport::Unix(
                "unix:///run/docker.sock".to_string()
            ))
        );
    }

    #[test]
    fn unknown_or_empty_docker_host_keeps_the_old_behaviour() {
        for value in [
            None,
            Some(""),
            Some("   "),
            Some("npipe:////./pipe/docker_engine"),
            Some("ssh://user@host"),
            Some("https://example.invalid:2376"),
            Some("/var/run/docker.sock"),
        ] {
            assert_eq!(docker_host_transport(value), None, "value: {value:?}");
        }
    }
}
