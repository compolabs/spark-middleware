
use futures_util::{SinkExt, StreamExt};
use log::{info, error};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::{tungstenite::protocol::Message, WebSocketStream};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_tungstenite::MaybeTlsStream;
use crate::config::settings::Settings;

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
    ) {
        if let Some(message) = ws_stream.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    match serde_json::from_str::<MatcherConnectRequest>(&text) {
                        Ok(connect_request) => {
                            if self.is_matcher_allowed(&connect_request.uuid) {
                                info!("Matcher with UUID {} connected", connect_request.uuid);
                                self.process_matcher(ws_stream, sender).await;
                            } else {
                                error!("Matcher with UUID {} is not allowed", connect_request.uuid);
                                let _ = ws_stream
                                    .send(Message::Text("Unauthorized".to_string()))
                                    .await;
                                let _ = ws_stream.close(None).await;
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse MatcherConnectRequest: {:?}, raw message: {}", e, text);
                            let _ = ws_stream
                                .send(Message::Text("Invalid request format".to_string()))
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
}

