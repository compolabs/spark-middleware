use futures_util::{SinkExt, StreamExt};
use log::{info, error};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::{tungstenite::protocol::Message, WebSocketStream};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_tungstenite::MaybeTlsStream;
use crate::{config::settings::Settings, indexer::spot_order::{OrderType, SpotOrder}, middleware::aggregator::Aggregator};

#[derive(Serialize, Deserialize)]
pub enum MatcherRequest {
    Orders(Vec<SpotOrder>), 
}

#[derive(Serialize, Deserialize)]
pub enum MatcherResponse {
    MatchResult { success: bool, matched_orders: Vec<String> }, 
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
        ws_stream: WebSocketStream<TcpStream>,
        sender: mpsc::Sender<String>,
        aggregator: Arc<Aggregator>,
    ) {
        let mut ws_stream = ws_stream; 

        if let Some(message) = ws_stream.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Ok(MatcherConnectRequest { uuid }) = serde_json::from_str(&text) {
                        if self.is_matcher_allowed(&uuid) {
                            info!("Matcher with UUID {} connected", uuid);

                            let aggregated_orders = aggregator.get_all_aggregated_orders().await;
                            if let Err(e) = self.send_orders_to_matcher(&mut ws_stream, aggregated_orders).await {
                                error!("Failed to send orders to matcher: {}", e);
                                return;
                            }

                            if let Some(response) = self.receive_matcher_response(&mut ws_stream).await {
                                aggregator.process_matcher_response(response).await;
                            }
                        }
                    }
                }
                _ => {
                    error!("Unexpected message format from matcher");
                }
            }
        }
    }

    fn is_matcher_allowed(&self, uuid: &str) -> bool {
        self.settings.matchers.matchers.contains(&uuid.to_string())
    }

    async fn process_matcher(
        &self,
        mut ws_stream: WebSocketStream<TcpStream>,
        sender: mpsc::Sender<String>,  
    ) {
        while let Some(message) = ws_stream.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    info!("Received message from matcher: {}", text);
                    if let Err(e) = sender.send(text).await {
                        error!("Failed to send message to handler: {:?}", e);
                    }
                }
                Err(e) => {
                    error!("Error receiving message: {:?}", e);
                    break;
                }
                _ => {}
            }
        }
    }

    pub async fn send_orders_to_matcher(
        &self,
        ws_stream: &mut WebSocketStream<TcpStream>,
        orders: Vec<SpotOrder>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let request = MatcherRequest::Orders(orders);
        let request_text = serde_json::to_string(&request)?;
        
        ws_stream.send(Message::Text(request_text)).await?;
        Ok(())
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
}

