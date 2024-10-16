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

use super::types::MatcherOrderUpdate;

pub struct MatcherWebSocket {
    pub settings: Arc<Settings>,
    pub order_book: Arc<OrderBook>,
    pub matching_orders: Arc<Mutex<HashSet<String>>>,  
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

        
        while let Some(Ok(message)) = read.next().await {
            if let Message::Text(text) = message {
                match serde_json::from_str::<MatcherRequest>(&text) {
                    Ok(MatcherRequest::BatchRequest(_)) => {
                        self.handle_batch_request(&mut write).await;
                    }
                    Ok(MatcherRequest::OrderUpdates(order_updates)) => {
                        self.handle_order_updates(order_updates).await;
                    }
                    _ => {
                        error!("{:?}",text);
                        error!("Invalid message format received");
                    }
                }
            }
        }
    }

    async fn handle_batch_request(
        &self,
        write: &mut futures_util::stream::SplitSink<
            WebSocketStream<TcpStream>,
            Message,
        >,
    ) {
        
        let batch_size = self.settings.matchers.batch_size;
        let available_orders = self.get_available_orders(batch_size).await;

        let response = if available_orders.is_empty() {
            MatcherResponse::NoOrders
        } else {
            MatcherResponse::Batch(available_orders)
        };
        
        let response_text = serde_json::to_string(&response).unwrap();

        if let Err(e) = write.send(Message::Text(response_text)).await {
            error!("Failed to send response to matcher: {}", e);
        }
    }

    async fn handle_order_updates(
        &self,
        order_updates: Vec<MatcherOrderUpdate>,
    ) {
        let mut matching_orders = self.matching_orders.lock().await;

        for update in order_updates {
            matching_orders.remove(&update.order_id);
        }
    }

    async fn get_available_orders(&self, batch_size: usize) -> Vec<SpotOrder> {
        let mut available_orders = Vec::new();

        // Получаем снимок matching_orders
        let matching_order_ids = {
            let matching_orders = self.matching_orders.lock().await;
            matching_orders.clone() // Предполагается, что matching_orders: HashSet<String>
        };
        // matching_order_ids теперь имеет тип HashSet<String>

        // Получаем копии buy_orders и sell_orders
        let buy_orders = {
            let buy_orders_lock = self.order_book.get_buy_orders();
            buy_orders_lock
                .values()
                .flat_map(|orders| orders.iter().cloned())
                .collect::<Vec<_>>()
        };

        let sell_orders = {
            let sell_orders_lock = self.order_book.get_sell_orders();
            sell_orders_lock
                .values()
                .flat_map(|orders| orders.iter().cloned())
                .collect::<Vec<_>>()
        };

        // Теперь мы освободили RwLockReadGuard и можем продолжать без проблем

        // Фильтруем ордера, исключая те, которые уже находятся в matching_orders
        let mut buy_queue: Vec<SpotOrder> = buy_orders
            .into_iter()
            .filter(|order| !matching_order_ids.contains(&order.id))
            .collect();

        let mut sell_queue: Vec<SpotOrder> = sell_orders
            .into_iter()
            .filter(|order| !matching_order_ids.contains(&order.id))
            .collect();

        // Сортируем очереди
        buy_queue.sort_by_key(|o| (std::cmp::Reverse(o.price), o.timestamp));
        sell_queue.sort_by_key(|o| (o.price, o.timestamp));

        let mut buy_index = 0;
        let mut sell_index = 0;

        let mut new_matching_order_ids = Vec::new();

        while buy_index < buy_queue.len()
            && sell_index < sell_queue.len()
            && available_orders.len() < batch_size
        {
            let buy_order = &buy_queue[buy_index];
            let sell_order = &sell_queue[sell_index];

            // Проверяем, подходит ли цена
            if buy_order.price >= sell_order.price {
                // Определяем объем исполнения
                let trade_amount = std::cmp::min(buy_order.amount, sell_order.amount);

                // Создаем новые ордера с объемом исполнения
                let mut matched_buy_order = buy_order.clone();
                matched_buy_order.amount = trade_amount;

                let mut matched_sell_order = sell_order.clone();
                matched_sell_order.amount = trade_amount;

                // Добавляем ордера в батч
                available_orders.push(matched_buy_order);
                available_orders.push(matched_sell_order);

                // Добавляем идентификаторы ордеров в new_matching_order_ids
                new_matching_order_ids.push(buy_order.id.clone());
                new_matching_order_ids.push(sell_order.id.clone());

                // Обновляем объемы в очередях
                buy_queue[buy_index].amount -= trade_amount;
                sell_queue[sell_index].amount -= trade_amount;

                // Если объем ордера исчерпан, переходим к следующему
                if buy_queue[buy_index].amount == 0 {
                    buy_index += 1;
                }

                if sell_queue[sell_index].amount == 0 {
                    sell_index += 1;
                }

                if available_orders.len() >= batch_size {
                    break;
                }
            } else {
                // Цены не пересекаются, переходим к следующему покупателю
                buy_index += 1;
            }
        }

        // Теперь обновляем matching_orders, добавляя новые идентификаторы
        {
            let mut matching_orders = self.matching_orders.lock().await;
            for order_id in new_matching_order_ids {
                matching_orders.insert(order_id);
            }
        }

        available_orders
    }


    pub async fn update_order(&self, order: SpotOrder) {
        self.order_book.update_order(order.clone());
        
        
        let mut matching_orders = self.matching_orders.lock().await;
        matching_orders.remove(&order.id);
    }
}
