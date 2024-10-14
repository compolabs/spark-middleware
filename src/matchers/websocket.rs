use crate::config::settings::Settings;
use crate::matchers::types::{MatcherRequest, MatcherResponse};
use crate::storage::order_book::OrderBook;
use crate::indexer::spot_order::{OrderType, SpotOrder};
use futures_util::{StreamExt, SinkExt};
use log::error;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::WebSocketStream;
use tokio::net::TcpStream;

pub struct MatcherWebSocket {
    pub settings: Arc<Settings>,
    pub order_book: Arc<OrderBook>,
    pub matching_orders: Arc<Mutex<HashSet<String>>>,  // Храним ID ордеров, которые в процессе матчинга
}

impl MatcherWebSocket {
    pub fn new(settings: Arc<Settings>, order_book: Arc<OrderBook>) -> Self {
        Self {
            settings,
            order_book,
            matching_orders: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub async fn handle_connection(
        self: Arc<Self>,
        ws_stream: WebSocketStream<TcpStream>,
    ) {
        let (mut write, mut read) = ws_stream.split();

        // Основной цикл обработки сообщений от клиента
        while let Some(Ok(message)) = read.next().await {
            if let Message::Text(text) = message {
                match serde_json::from_str::<MatcherRequest>(&text) {
                    Ok(MatcherRequest::BatchRequest(_)) => {
                        // Формируем батч ордеров для матчинга
                        let batch_size = self.settings.matchers.batch_size;
                        let available_orders = self.get_available_orders(batch_size).await;
                        
                        let response = MatcherResponse::Batch(available_orders);
                        let response_text = serde_json::to_string(&response).unwrap();
                        
                        if let Err(e) = write.send(Message::Text(response_text)).await {
                            error!("Failed to send batch to matcher: {}", e);
                        }
                    }
                    _ => {
                        error!("Invalid message format received");
                    }
                }
            }
        }
    }

    // Метод для получения батча ордеров, которые не находятся в процессе матчинга
    async fn get_available_orders(&self, batch_size: usize) -> Vec<SpotOrder> {
        let mut matching_orders = self.matching_orders.lock().await;
        let mut available_orders = Vec::new();

        // Получаем все ордера из OrderBook, которые не в MatchingOrders
        let orders = self.order_book.get_orders_in_range(0, u128::MAX, OrderType::Buy)
            .into_iter()
            .chain(self.order_book.get_orders_in_range(0, u128::MAX, OrderType::Sell))
            .filter(|order| !matching_orders.contains(&order.id))
            .take(batch_size)
            .collect::<Vec<_>>();

        // Добавляем ордера в matching_orders
        for order in &orders {
            matching_orders.insert(order.id.clone());
        }

        available_orders
    }

    // Метод для обработки обновления ордера
    pub async fn update_order(&self, order: SpotOrder) {
        self.order_book.update_order(order.clone());
        
        // Удаляем из MatchingOrders, если он там есть
        let mut matching_orders = self.matching_orders.lock().await;
        matching_orders.remove(&order.id);
    }
}
