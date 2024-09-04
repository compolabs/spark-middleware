use async_tungstenite::tokio::{connect_async, TokioAdapter};
use async_tungstenite::tungstenite::protocol::Message;
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use url::Url;
use crate::{error::Error, indexer::spot_order::SpotOrder};

pub struct WebSocketClientSubsquid {
    pub url: Url,
}

impl WebSocketClientSubsquid {
    pub fn new(url: Url) -> Self {
        WebSocketClientSubsquid { url }
    }

    pub async fn connect(
        &self,
        sender: mpsc::Sender<SpotOrder>,
    ) -> Result<(), Error> {
        loop {
            let mut ws_stream = match self.connect_to_ws().await {
                Ok(ws_stream) => ws_stream,
                Err(e) => {
                    error!("Failed to establish websocket connection: {:?}", e);
                    continue;
                }
            };

            info!("WebSocket connected to Subsquid");

            ws_stream
                .send(Message::Text(r#"{"type": "connection_init", "payload": {}}"#.into()))
                .await
                .expect("Failed to send init message");

            info!("Connection init message sent, waiting for connection_ack...");

            let mut last_data_time = tokio::time::Instant::now();
            let mut initialized = false;

            while let Some(message) = ws_stream.next().await {
                if last_data_time.elapsed() > tokio::time::Duration::from_secs(60) {
                    error!("No data messages received for the last 60 seconds, reconnecting...");
                    break;
                }

                match message {
                    Ok(Message::Text(text)) => {
                        info!("Received message: {}", text);

                        if text.contains("\"type\":\"connection_ack\"") {
                            initialized = true;
                            info!("Connection established, subscribing to data...");

                            let subscription_query = r#"
                            subscription {
                                orders(limit: 5, orderBy: timestamp_DESC) {
                                    id
                                }
                            }"#;

                            let subscription_message = serde_json::json!({
                                "id": "1",
                                "type": "subscribe",
                                "payload": {
                                    "query": subscription_query,
                                    "variables": {}
                                }
                            });

                            ws_stream
                                .send(Message::Text(subscription_message.to_string()))
                                .await
                                .expect("Failed to send subscription message");

                            info!("Subscription message sent.");
                        }

                        if text.contains("\"type\":\"next\"") {
                            info!("Received subscription data: {}", text);

                            if let Ok(response) = serde_json::from_str::<serde_json::Value>(&text) {
                                info!("Parsed response: {:?}", response);
                            } else {
                                error!("Failed to deserialize WebSocket response: {:?}", text);
                            }
                        }

                        last_data_time = tokio::time::Instant::now();
                    }
                    Ok(Message::Close(frame)) => {
                        if let Some(frame) = frame {
                            error!("WebSocket connection closed by server: code = {}, reason = {}", frame.code, frame.reason);
                        } else {
                            error!("WebSocket connection closed by server without a frame.");
                        }
                        break;
                    }
                    Err(e) => {
                        error!("Error in websocket connection: {:?}", e);
                        break;
                    }
                    _ => {}
                }

                if initialized {
                    last_data_time = tokio::time::Instant::now();
                }
            }
        }
    }

    async fn connect_to_ws(
        &self,
    ) -> Result<async_tungstenite::WebSocketStream<TokioAdapter<TcpStream>>, Box<dyn std::error::Error>> {
        let url = self.url.to_string();

        let request = async_tungstenite::tungstenite::handshake::client::Request::builder()
            .uri(url)
            .header("Sec-WebSocket-Protocol", "graphql-transport-ws") // Указываем нужный протокол
            .body(())
            .unwrap();

        let (ws_stream, _) = connect_async(request).await?;

        Ok(ws_stream)
    }
}
