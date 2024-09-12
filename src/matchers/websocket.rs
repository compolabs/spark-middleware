use crate::{
    config::settings::Settings, indexer::spot_order::SpotOrder, middleware::aggregator::Aggregator,
};
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::{tungstenite::protocol::Message, WebSocketStream};

#[derive(Serialize, Deserialize)]
pub enum MatcherRequest {
    Orders(Vec<SpotOrder>),
}

#[derive(Serialize, Deserialize)]
pub struct MatcherResponse {
    pub success: bool,
    pub matched_orders: Vec<String>,
    pub error_message: Option<String>,
}

#[derive(Deserialize)]
struct MatcherConnectRequest {
    uuid: String,
}

pub struct MatcherWebSocket {
    pub settings: Arc<Settings>,
}

impl MatcherWebSocket {
    pub fn new(settings: Arc<Settings>) -> Self {
        Self { settings }
    }

    pub async fn handle_connection(
        &self,
        mut ws_stream: WebSocketStream<TcpStream>,
        sender: mpsc::Sender<String>,
        aggregator: Arc<Aggregator>,
    ) {
        if let Some(message) = ws_stream.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Ok(MatcherConnectRequest { uuid }) = serde_json::from_str(&text) {
                        if self.is_matcher_allowed(&uuid) {
                            info!("Matcher with UUID {} connected", uuid);

                            let matching_batches = aggregator.select_matching_batches(20).await;

                            if matching_batches.is_empty() {
                                info!("No matching orders found.");
                                let _ = sender.send("No matching orders found.".to_string()).await;
                                return;
                            }

                            self.process_batches(
                                matching_batches,
                                &mut ws_stream,
                                sender,
                                aggregator,
                            )
                            .await;
                        }
                    }
                }
                _ => {
                    error!("Unexpected message format from matcher");
                }
            }
        }
    }

    async fn process_batches(
        &self,
        batches: Vec<Vec<(SpotOrder, SpotOrder)>>,
        ws_stream: &mut WebSocketStream<TcpStream>,
        sender: mpsc::Sender<String>,
        aggregator: Arc<Aggregator>,
    ) {
        for batch in batches {
            let flat_batch: Vec<SpotOrder> = batch
                .into_iter()
                .flat_map(|(buy_order, sell_order)| vec![buy_order, sell_order])
                .collect();

            if let Err(e) = self.send_batch_to_matcher(ws_stream, flat_batch).await {
                error!("Failed to send batch to matcher: {}", e);
                return;
            }

            if let Some(response) = self.receive_matcher_response(ws_stream).await {
                self.process_matcher_response(response, &aggregator, sender.clone())
                    .await;
            } else {
                error!("No response from matcher");
                break;
            }
        }
    }

    async fn send_batch_to_matcher(
        &self,
        ws_stream: &mut WebSocketStream<TcpStream>,
        batch: Vec<SpotOrder>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let request = MatcherRequest::Orders(batch);
        let request_text = serde_json::to_string(&request)?;

        ws_stream.send(Message::Text(request_text)).await?;
        Ok(())
    }

    async fn process_matcher_response(
        &self,
        response: MatcherResponse,
        _aggregator: &Arc<Aggregator>,
        _sender: mpsc::Sender<String>,
    ) {
        if response.success {
            info!(
                "Matcher successfully processed orders: {:?}",
                response.matched_orders
            );
        } else {
            error!("Matcher failed to process orders");
            error!("Matcher error {:?}", response.error_message);
        }
    }

    pub async fn receive_matcher_response(
        &self,
        ws_stream: &mut WebSocketStream<TcpStream>,
    ) -> Option<MatcherResponse> {
        while let Some(message) = ws_stream.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Ok(response) = serde_json::from_str::<MatcherResponse>(&text) {
                        return Some(response);
                    }
                }
                _ => {
                    error!("Received unexpected message from matcher.");
                }
            }
        }
        None
    }

    fn is_matcher_allowed(&self, uuid: &str) -> bool {
        self.settings.matchers.matchers.contains(&uuid.to_string())
    }
}
