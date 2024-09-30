use crate::config::settings::Settings;
use crate::indexer::spot_order::SpotOrder;
use crate::metrics::types::OrderMetrics;
use crate::middleware::aggregator::Aggregator;
use crate::matchers::metrics_handler::MetricsHandler;
use crate::matchers::manager::MatcherManager;
use crate::matchers::types::MatcherResponse;
use crate::error::Error;
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::protocol::Message, WebSocketStream};

use super::types::{MatcherRequest, MatcherResponseWrapper};
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

        while let Some(message) = ws_stream_lock.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    
                    if let Ok(wrapper) = serde_json::from_str::<MatcherResponseWrapper>(&text) {
                        return Some(wrapper.MatchResult);  
                    } else {
                        error!("Failed to parse message into MatcherResponseWrapper");
                    }
                }
                _ => {
                    error!("Received unexpected message from matcher.");
                }
            }
        }

        None 
    }

    pub async fn process_batches(
        self: Arc<Self>,
        uuid: String,
        ws_stream: Arc<Mutex<WebSocketStream<TcpStream>>>, 
        sender: mpsc::Sender<String>,
        aggregator: Arc<Aggregator>,
        matcher_manager: Arc<Mutex<MatcherManager>>,
    ) {
        let batch_size = 20;
        let mut in_flight_batches = 0; 
        let max_in_flight_batches = 1; 

        loop {
            if in_flight_batches >= max_in_flight_batches {
                
                let duration = Duration::from_millis(100);
                tokio::time::sleep(duration).await;
                continue;
            }

            let batch_to_process = {
                let mut manager = matcher_manager.lock().await;
                if manager.select_matcher() == Some(uuid.clone()) {
                    aggregator.select_matching_batches(batch_size).await
                } else {
                    Vec::new()
                }
            };

            if batch_to_process.is_empty() {
                info!("No matching orders found for matcher {}.", uuid);
                let _ = sender.send(format!("No matching orders found for matcher {}.", uuid)).await;
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                continue;
            }

            for single_batch in batch_to_process {
                let self_clone = Arc::clone(&self);
                let ws_stream_clone = ws_stream.clone();
                let sender_clone = sender.clone();
                let aggregator_clone = aggregator.clone();
                let matcher_manager_clone = matcher_manager.clone();
                let uuid_clone = uuid.clone();

                in_flight_batches += 1;

                tokio::spawn(async move {
                    if let Err(e) = self_clone.process_single_batch(
                        ws_stream_clone, 
                        sender_clone,
                        aggregator_clone,
                        matcher_manager_clone,
                        uuid_clone.clone(),
                        single_batch,
                    ).await {
                        error!("Failed to process batch for matcher {}: {}", uuid_clone, e);
                    }

                    
                    in_flight_batches -= 1;
                });
            }
        }
    }

    async fn process_single_batch(
        &self,
        ws_stream: Arc<Mutex<WebSocketStream<TcpStream>>>, 
        sender: mpsc::Sender<String>,
        aggregator: Arc<Aggregator>,
        matcher_manager: Arc<Mutex<MatcherManager>>,
        uuid: String,
        batch: Vec<(SpotOrder, SpotOrder)>,
    ) -> Result<(), Error> {
        let flat_batch: Vec<SpotOrder> = batch
            .into_iter()
            .flat_map(|(buy_order, sell_order)| vec![buy_order, sell_order])
            .collect();

        info!("Sending batch of {} orders to matcher {}", flat_batch.len(), uuid);

        if let Err(e) = self.send_batch_to_matcher(ws_stream.clone(), flat_batch).await {
            error!("Failed to send batch to matcher {}: {}", uuid, e);
            return Err(Error::SendingToMatcherError);
        }

        
        if let Some(response) = self.receive_matcher_response(ws_stream.clone()).await {
            self.metrics_handler.process_matcher_response(response, &aggregator, sender.clone(), &uuid).await;
            matcher_manager.lock().await.increase_load(&uuid, 1); 
        } else {
            error!("No response from matcher {}", uuid);
        }

        
        let mut busy = self.busy.lock().await;
        *busy = false;

        Ok(())
    }
}
