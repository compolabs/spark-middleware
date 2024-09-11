use futures_util::{SinkExt, StreamExt};
use log::{info, error};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::{tungstenite::protocol::Message, WebSocketStream};
use std::sync::Arc;
use tokio::net::TcpStream;
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

                            // Теперь мы вызываем функцию select_matching_batches для получения подходящих батчей
                            let matching_batches = aggregator.select_matching_batches(20).await;

                            if matching_batches.is_empty() {
                                info!("No matching orders found.");
                                let _ = sender.send("No matching orders found.".to_string()).await;
                                return;
                            }

                            // Передаем батчи по очереди
                            self.process_batches(matching_batches, &mut ws_stream, sender, aggregator).await;
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
        batches: Vec<Vec<(SpotOrder, SpotOrder)>>,  // Теперь это пары ордеров для матча
        ws_stream: &mut WebSocketStream<TcpStream>,
        sender: mpsc::Sender<String>,
        aggregator: Arc<Aggregator>,
    ) {
        for batch in batches {
            // Преобразуем пары ордеров в список для отправки матчеру
            let flat_batch: Vec<SpotOrder> = batch
                .into_iter()
                .flat_map(|(buy_order, sell_order)| vec![buy_order, sell_order])
                .collect();

            if let Err(e) = self.send_batch_to_matcher(ws_stream, flat_batch).await {
                error!("Failed to send batch to matcher: {}", e);
                //let _ = sender.send(format!("Failed to send batch: {}", e)).await;
                return;
            }

            if let Some(response) = self.receive_matcher_response(ws_stream).await {
                self.process_matcher_response(response, &aggregator, sender.clone()).await;
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
        aggregator: &Arc<Aggregator>,
        sender: mpsc::Sender<String>,
    ) {
        match response {
            MatcherResponse::MatchResult { success, matched_orders } => {
                if success {
                    info!("Matcher successfully processed orders: {:?}", matched_orders);
                    let _ = sender.send("Matcher successfully processed orders".to_string()).await;

                    aggregator.remove_matched_orders(matched_orders).await;
                } else {
                    error!("Matcher failed to process orders");
//                    let _ = sender.send("Matcher failed to process orders".to_string()).await;
                }
            }
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

