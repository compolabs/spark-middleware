pub mod envio;
pub mod envio_event_handler;
pub mod pangea;
pub mod pangea_event_handler;
pub mod spot_order;

use crate::error::Error;
use async_trait::async_trait;
use tokio::task::JoinHandle;

#[async_trait]
pub trait Indexer: Send + Sync {
    async fn initialize(&self, tasks: &mut Vec<JoinHandle<()>>) -> Result<(), Error>;
}
