use crate::config::settings::Settings;
use crate::matchers::batch_processor::BatchProcessor;
use crate::matchers::types::{
    MatcherConnectRequest, MatcherRequest, MatcherResponse,
};
use crate::middleware::order_pool::ShardedOrderPool;
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::WebSocketStream;

pub struct MatcherWebSocket {
    pub settings: Arc<Settings>,
    pub batch_processor: Arc<BatchProcessor>,
}

impl MatcherWebSocket {
    pub fn new(settings: Arc<Settings>, batch_processor: Arc<BatchProcessor>) -> Self {
        Self {
            settings,
            batch_processor,
        }
    }

    pub async fn handle_connection(
        self: Arc<Self>,
        ws_stream: WebSocketStream<TcpStream>,
        order_pool: Arc<ShardedOrderPool>,
    ) {
        // Разделяем поток на write и read
        let (write, mut read) = ws_stream.split();

        // Первое сообщение должно быть идентификацией
        if let Some(Ok(message)) = read.next().await {
            if let Message::Text(text) = message {
                if let Ok(MatcherConnectRequest { uuid }) = serde_json::from_str(&text) {
                    info!("Matcher with UUID {} connected", uuid);

                    let self_clone = self.clone();
                    let order_pool_clone = order_pool.clone();

                    // Запускаем обработку сообщений в отдельной задаче
                    tokio::spawn(async move {
                        self_clone
                            .process_messages(uuid, write, read, order_pool_clone)
                            .await;
                    });
                } else {
                    error!("Invalid matcher connect request format");
                }
            } else {
                error!("Expected text message from matcher");
            }
        } else {
            error!("Expected identification message from matcher");
        }
    }

    async fn process_messages(
        &self,
        uuid: String,
        mut write: impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
        mut read: impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
        order_pool: Arc<ShardedOrderPool>,
    ) {
        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    match serde_json::from_str::<MatcherRequest>(&text) {
                        Ok(MatcherRequest::BatchRequest(_)) => {
                            // Формируем батч и отправляем матчеру
                            let batch_size = self.settings.matchers.batch_size;
                            if let Some(batch) = self
                                .batch_processor
                                .form_batch(batch_size, order_pool.clone())
                                .await
                            {
                                let response = MatcherResponse::Batch(batch);
                                let response_text = serde_json::to_string(&response).unwrap();
                                if let Err(e) = write.send(Message::Text(response_text)).await {
                                    error!("Failed to send batch to matcher {}: {}", uuid, e);
                                    break;
                                }
                            } else {
                                info!("No orders available to form a batch for matcher {}", uuid);
                                // Можно отправить пустой батч или специальное сообщение
                            }
                        }
                        Ok(MatcherRequest::OrderUpdates(order_updates)) => {
                            // Обновляем статусы ордеров в пуле
                            order_pool.update_order_status(order_updates).await;
                            // Отправляем подтверждение (опционально)
                            let ack = MatcherResponse::Ack;
                            let ack_text = serde_json::to_string(&ack).unwrap();
                            if let Err(e) = write.send(Message::Text(ack_text)).await {
                                error!("Failed to send ack to matcher {}: {}", uuid, e);
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse message from matcher {}: {}", uuid, e);
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    info!("Matcher {} disconnected", uuid);
                    break;
                }
                Err(e) => {
                    error!("WebSocket error with matcher {}: {}", uuid, e);
                    break;
                }
                _ => {
                    error!("Unexpected message format from matcher {}", uuid);
                }
            }
        }
    }
}
