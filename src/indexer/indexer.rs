use async_trait::async_trait;
use tokio::task::JoinHandle;
use crate::error::Error;

#[async_trait]
pub trait Indexer: Send + Sync {
    async fn initialize(&self, tasks: &mut Vec<JoinHandle<()>>) -> Result<(), Error>;
}
