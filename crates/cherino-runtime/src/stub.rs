use std::path::PathBuf;

fn not_available() -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "YoukiManager is only available on Linux",
    )
    .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootfsCapability {
    None,
    Overlay,
    Btrfs,
}

pub fn detect_inside_container() -> bool {
    false
}

pub fn detect_rootfs_capability(_: &PathBuf) -> RootfsCapability {
    RootfsCapability::None
}

pub struct YoukiManager;

impl YoukiManager {
    pub fn new(
        _data_dir: &std::path::Path,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Err(not_available())
    }

    pub async fn initialize(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err(not_available())
    }
}

fn err() -> cherino::errors::ContainerError {
    cherino::errors::ContainerError::OperationFailed {
        container_id: "n/a".into(),
        message: "YoukiManager not available on this platform".into(),
    }
}

#[async_trait::async_trait]
impl cherino::ops::ContainerOps for YoukiManager {
    async fn create(
        &self,
        _: &cherino::types::ContainerCreateParams,
    ) -> cherino::errors::ContainerResult<cherino::types::ContainerInfo> {
        Err(err())
    }
    async fn start(&self, _: &str) -> cherino::errors::ContainerResult<()> {
        Err(err())
    }
    async fn stop(&self, _: &str) -> cherino::errors::ContainerResult<()> {
        Err(err())
    }
    async fn remove(&self, _: &str, _: bool) -> cherino::errors::ContainerResult<()> {
        Err(err())
    }
    async fn restart(&self, _: &str) -> cherino::errors::ContainerResult<()> {
        Err(err())
    }
    async fn list(&self) -> cherino::errors::ContainerResult<Vec<cherino::types::ContainerInfo>> {
        Ok(vec![])
    }
    async fn list_with_filter(
        &self,
        _: Option<&str>,
        _: Option<std::collections::HashMap<String, String>>,
        _: bool,
    ) -> cherino::errors::ContainerResult<Vec<cherino::types::ContainerInfo>> {
        Ok(vec![])
    }
    async fn inspect(
        &self,
        _: &str,
    ) -> cherino::errors::ContainerResult<cherino::types::ContainerDetail> {
        Err(err())
    }
    async fn is_running(&self, _: &str) -> cherino::errors::ContainerResult<bool> {
        Ok(false)
    }
    async fn exec(
        &self,
        _: &str,
        _: &[&str],
    ) -> cherino::errors::ContainerResult<cherino::types::ExecOutput> {
        Err(err())
    }
    async fn fork(
        &self,
        _: &cherino::types::ContainerForkParams,
    ) -> cherino::errors::ContainerResult<cherino::types::ContainerInfo> {
        Err(err())
    }
    async fn commit(
        &self,
        _: &str,
        _: &str,
        _: Option<&str>,
    ) -> cherino::errors::ContainerResult<String> {
        Err(err())
    }
    async fn commit_with_labels(
        &self,
        _: &str,
        _: &str,
        _: Option<&str>,
        _: Option<&std::collections::HashMap<String, String>>,
    ) -> cherino::errors::ContainerResult<String> {
        Err(err())
    }
    async fn wait_healthy(
        &self,
        _: &str,
        _: std::time::Duration,
    ) -> cherino::errors::ContainerResult<()> {
        Err(err())
    }
    async fn ensure_running(
        &self,
        _: &str,
    ) -> cherino::errors::ContainerResult<cherino::types::ContainerInfo> {
        Err(err())
    }
    async fn recreate(
        &self,
        _: &str,
        _: &str,
    ) -> cherino::errors::ContainerResult<cherino::types::ContainerInfo> {
        Err(err())
    }
    async fn list_images(
        &self,
    ) -> cherino::errors::ContainerResult<Vec<cherino::types::ImageInfo>> {
        Ok(vec![])
    }
    async fn pull_image(&self, _: &str) -> cherino::errors::ContainerResult<String> {
        Err(err())
    }
    async fn image_exists(&self, _: &str) -> cherino::errors::ContainerResult<bool> {
        Ok(false)
    }
    async fn remove_image(&self, _: &str, _: bool) -> cherino::errors::ContainerResult<()> {
        Err(err())
    }
    async fn remove_with_image(
        &self,
        _: &str,
        _: &str,
        _: bool,
    ) -> cherino::errors::ContainerResult<()> {
        Err(err())
    }
    async fn create_volume(&self, _: &str) -> cherino::errors::ContainerResult<String> {
        Err(err())
    }
    async fn remove_volume(&self, _: &str, _: bool) -> cherino::errors::ContainerResult<()> {
        Err(err())
    }
    async fn volume_exists(&self, _: &str) -> cherino::errors::ContainerResult<bool> {
        Ok(false)
    }
    async fn list_volumes(
        &self,
    ) -> cherino::errors::ContainerResult<Vec<cherino::types::DockerVolumeInfo>> {
        Ok(vec![])
    }
    async fn logs(&self, _: &str, _: usize) -> cherino::errors::ContainerResult<Vec<String>> {
        Ok(vec![])
    }
    async fn get_container_logs(
        &self,
        _: &str,
        _: usize,
    ) -> cherino::errors::ContainerResult<String> {
        Ok(String::new())
    }
    async fn writable_rootfs(
        &self,
        _: &str,
    ) -> cherino::errors::ContainerResult<cherino::types::WritableRootfs> {
        Err(err())
    }
    async fn diff_workspace(
        &self,
        _: &str,
        _: &std::path::Path,
        _: &str,
    ) -> cherino::errors::ContainerResult<Vec<cherino::types::PathChange>> {
        Ok(vec![])
    }
    async fn download_archive(
        &self,
        _: &str,
        _: &str,
    ) -> cherino::errors::ContainerResult<Vec<u8>> {
        Err(err())
    }
    async fn upload_archive(
        &self,
        _: &str,
        _: &str,
        _: Vec<u8>,
    ) -> cherino::errors::ContainerResult<()> {
        Err(err())
    }
    fn clone_boxed(&self) -> Box<dyn cherino::ops::ContainerOps> {
        Box::new(YoukiManager)
    }
}
