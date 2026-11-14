use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContainerError {
    #[cfg(feature = "docker")]
    #[error("Docker API error: {0}")]
    DockerApi(#[from] bollard::errors::Error),

    #[error("Youki/libcontainer error: {0}")]
    YoukiApi(String),

    #[error("Container not found: {0}")]
    NotFound(String),

    #[error("Container already exists: {0}")]
    AlreadyExists(String),

    #[error("Invalid parameter: {0}")]
    InvalidParam(String),

    #[error("Container operation failed: {container_id}: {message}")]
    OperationFailed {
        container_id: String,
        message: String,
    },

    /// The container was created, but the security options it actually runs
    /// with are not the ones the profile requested. The container is torn down
    /// before this is returned: a sandbox with a silently downgraded profile
    /// must never be handed to a caller.
    #[error("Security profile drift on {container_id}: {drift}")]
    SecurityProfileDrift {
        container_id: String,
        /// Keys only — never the seccomp profile body.
        drift: String,
    },

    #[error("Commit failed: {0}")]
    CommitFailed(String),

    #[error("Exec failed in container {container_id}: {message}")]
    ExecFailed {
        container_id: String,
        message: String,
    },

    #[error("Image operation failed: {0}")]
    ImageFailed(String),

    #[error("Volume operation failed: {0}")]
    VolumeFailed(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Unsupported backend: {0}")]
    UnsupportedBackend(String),

    #[error("Container runtime binary not found: {0}")]
    RuntimeNotFound(String),

    #[error("CLI command failed ({binary} {args}): {message}")]
    CliFailed {
        binary: String,
        args: String,
        message: String,
    },

    #[error("CLI output parse error: {0}")]
    CliParse(String),

    #[error("Operation not supported by this container runtime: {0}")]
    NotSupported(String),
}

pub type ContainerResult<T> = Result<T, ContainerError>;
