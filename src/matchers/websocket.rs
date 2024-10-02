use crate::config::settings::Settings;
use crate::matchers::batch_processor::BatchProcessor;
use crate::matchers::manager::MatcherManager;
use crate::matchers::metrics_handler::MetricsHandler;
use crate::matchers::types::{MatcherConnectRequest, MatcherResponse};
use crate::metrics::types::OrderMetrics;
use crate::middleware::aggregator::Aggregator;
use crate::middleware::order_pool::ShardedOrderPool;
use futures_util::StreamExt;
use log::{error, info};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{tungstenite::protocol::Message, WebSocketStream};

pub struct MatcherWebSocket {
    pub settings: Arc<Settings>,
    pub metrics_handler: MetricsHandler,
}

impl MatcherWebSocket {
    pub fn new(settings: Arc<Settings>, metrics: Arc<Mutex<OrderMetrics>>) -> Self {
        Self {
            settings,
            metrics_handler: MetricsHandler::new(metrics),
        }
    }

    pub async fn handle_connection(
        self: Arc<Self>,
        ws_stream: WebSocketStream<TcpStream>,
        sender: mpsc::Sender<String>,
        order_pool: Arc<ShardedOrderPool>, // Заменяем aggregator на пул
        matcher_manager: Arc<Mutex<MatcherManager>>,
    ) {
        let ws_stream = Arc::new(Mutex::new(ws_stream));

        if let Some(message) = ws_stream.lock().await.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Ok(MatcherConnectRequest { uuid }) = serde_json::from_str(&text) {
                        if self.is_matcher_allowed(&uuid) {
                            info!("Matcher with UUID {} connected", uuid);

                            {
                                let mut manager = matcher_manager.lock().await;
                                manager.add_matcher(uuid.clone());
                                self.log_active_matchers(&manager);
                            }

                            let self_arc = self.clone();
                            let ws_stream_clone = ws_stream.clone();
                            let sender_clone = sender.clone();
                            let order_pool_clone = Arc::clone(&order_pool); // Передаем пул ордеров

                            tokio::spawn(async move {
                                self_arc
                                    .start_batch_processing(
                                        uuid.clone(),
                                        ws_stream_clone,
                                        sender_clone,
                                        order_pool_clone, // Пул ордеров передаем сюда
                                        matcher_manager,
                                    )
                                    .await;
                            });
                        } else {
                            error!("Matcher with UUID {} is not allowed", uuid);
                        }
                    } else {
                        error!("Invalid matcher connect request format");
                    }
                }
                _ => {
                    error!("Unexpected message format from matcher.");
                }
            }
        };
    }

    async fn start_batch_processing(
        self: Arc<Self>,
        uuid: String,
        ws_stream: Arc<Mutex<WebSocketStream<TcpStream>>>,
        sender: mpsc::Sender<String>,
        order_pool: Arc<ShardedOrderPool>,
        matcher_manager: Arc<Mutex<MatcherManager>>,
    ) {
        let batch_processor =
            BatchProcessor::new(self.settings.clone(), self.metrics_handler.clone());
        batch_processor
            .process_batches(uuid, ws_stream, sender, order_pool, matcher_manager)
            .await;
    }

    fn is_matcher_allowed(&self, uuid: &str) -> bool {
        self.settings.matchers.matchers.contains(&uuid.to_string())
    }

    fn log_active_matchers(&self, manager: &MatcherManager) {
        let active_matchers: Vec<String> = manager.matchers.keys().cloned().collect();
        info!("Active matchers: {:?}", active_matchers);
    }
}
