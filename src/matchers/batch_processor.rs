use crate::config::settings::Settings;
use crate::error::Error;
use crate::indexer::spot_order::SpotOrder;
use crate::matchers::manager::MatcherManager;
use crate::matchers::metrics_handler::MetricsHandler;
use crate::matchers::types::MatcherResponse;
use crate::metrics::types::OrderMetrics;
use crate::middleware::order_pool::ShardedOrderPool;
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{tungstenite::protocol::Message, WebSocketStream};

use super::types::{MatcherRequest, MatcherResponseWrapper};
use uuid::Uuid;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BatchStatus {
    Pending,    
    InFlight,   
    Matched,    
    Failed,     
}


#[derive(Debug, Clone)]
pub struct Batch {
    pub id: String,                 
    pub buy_orders: Vec<SpotOrder>, 
    pub sell_orders: Vec<SpotOrder>, 
    pub status: BatchStatus,        
}

impl Batch {
    
    pub fn new(buy_orders: Vec<SpotOrder>, sell_orders: Vec<SpotOrder>) -> Self {
        Batch {
            id: Uuid::new_v4().to_string(), 
            buy_orders,
            sell_orders,
            status: BatchStatus::Pending,   
        }
    }

    
    pub fn is_empty(&self) -> bool {
        self.buy_orders.is_empty() || self.sell_orders.is_empty()
    }
}

pub struct BatchProcessor {
    pub settings: Arc<Settings>,
    pub metrics_handler: MetricsHandler,
    pub busy: Arc<Mutex<bool>>, 
}

impl BatchProcessor {
    
    pub fn new(settings: Arc<Settings>, metrics_handler: MetricsHandler) -> Arc<Self> {
        Arc::new(Self {
            settings,
            metrics_handler,
            busy: Arc::new(Mutex::new(false)),
        })
    }

    
    async fn send_batch_to_matcher(
        &self,
        ws_stream: Arc<Mutex<WebSocketStream<TcpStream>>>,
        batch: Vec<SpotOrder>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let request = MatcherRequest::Orders(batch); 
        let request_text = serde_json::to_string(&request)?;

        let mut ws_stream_lock = ws_stream.lock().await;
        ws_stream_lock.send(Message::Text(request_text)).await?; 

        Ok(())
    }

    async fn receive_matcher_response(
        &self,
        ws_stream: Arc<Mutex<WebSocketStream<TcpStream>>>,
    ) -> Option<MatcherResponse> {
        let mut ws_stream_lock = ws_stream.lock().await;
        let mut ws_stream_next = ws_stream_lock.next();

        // Set a timeout duration (e.g., 5 seconds)
        let duration = Duration::from_secs(5);

        while let Ok(Some(message)) = tokio::time::timeout(duration, ws_stream_next).await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Ok(response) = serde_json::from_str::<MatcherResponse>(&text) {
                        return Some(response);
                    } else {
                        error!("Failed to parse message into MatcherResponse");
                    }
                }
                _ => {
                    error!("Received unexpected message from matcher.");
                }
            }
            ws_stream_next = ws_stream_lock.next();
        }

        error!("Timeout while waiting for matcher response");
        None
    }

    pub async fn process_batches(
        self: Arc<Self>,
        uuid: String,
        ws_stream: Arc<Mutex<WebSocketStream<TcpStream>>>,
        sender: mpsc::Sender<String>,
        order_pool: Arc<ShardedOrderPool>, 
        matcher_manager: Arc<Mutex<MatcherManager>>,
    ) {
        let batch_size = 20;

        loop {
            
            let active_batches = {
                let manager = matcher_manager.lock().await;
                manager.get_active_batches(&uuid)
            };

            if active_batches >= 10 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            
            let batch_to_process = order_pool.select_batches(batch_size).await;

            if batch_to_process.is_empty() {
                info!("No matching orders found for matcher {}.", uuid);
                let _ = sender
                    .send(format!("No matching orders found for matcher {}.", uuid))
                    .await;
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                continue;
            }

            for single_batch in batch_to_process {
                let self_clone = Arc::clone(&self);
                let ws_stream_clone = ws_stream.clone();
                let sender_clone = sender.clone();
                let order_pool_clone = order_pool.clone();
                let matcher_manager_clone = matcher_manager.clone();
                let uuid_clone = uuid.clone();

                matcher_manager_clone.lock().await.increase_active_batches(&uuid);

                tokio::spawn(async move {
                    let matcher_manager_clone = matcher_manager_clone.clone();

                    if let Err(e) = self_clone
                        .process_single_batch(
                            ws_stream_clone,
                            sender_clone,
                            order_pool_clone,
                            matcher_manager_clone.clone(),
                            uuid_clone.clone(),
                            single_batch,
                        )
                        .await
                    {
                        error!("Failed to process batch for matcher {}: {}", uuid_clone, e);
                    }

                    matcher_manager_clone.lock().await.decrease_active_batches(&uuid_clone);
                });
            }
        }
    }

    
    async fn process_single_batch(
        &self,
        ws_stream: Arc<Mutex<WebSocketStream<TcpStream>>>,
        sender: mpsc::Sender<String>,
        order_pool: Arc<ShardedOrderPool>,
        matcher_manager: Arc<Mutex<MatcherManager>>,
        uuid: String,
        batch: Batch,
    ) -> Result<(), Error> {
        let flat_batch: Vec<SpotOrder> = batch
            .buy_orders
            .into_iter()
            .chain(batch.sell_orders.into_iter())
            .collect();

        info!(
            "Sending batch of {} orders to matcher {}",
            flat_batch.len(),
            uuid
        );

        if let Err(e) = self.send_batch_to_matcher(ws_stream.clone(), flat_batch).await {
            error!("Failed to send batch to matcher {}: {}", uuid, e);
            return Err(Error::SendingToMatcherError);
        }

        if let Some(response) = self.receive_matcher_response(ws_stream.clone()).await {
            order_pool.update_order_status(response.orders).await;
            matcher_manager.lock().await.increase_load(&uuid, 1);
        } else {
            error!("No response from matcher {} within timeout", uuid);
            // Handle the timeout case, e.g., retry or mark orders as failed
            // You can implement retry logic here if needed
        }

        Ok(())
    }
}
