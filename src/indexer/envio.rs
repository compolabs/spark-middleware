use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{net::TcpStream, sync::mpsc, time::Instant};
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use url::Url;

use crate::{
    indexer::spot_order::{OrderType, SpotOrder, WebSocketResponse},
    subscription::envio::format_graphql_subscription,
};

pub struct WebSocketClient {
    pub url: Url,
}

impl WebSocketClient {
    pub fn new(url: Url) -> Self {
        WebSocketClient { url }
    }

    pub async fn connect(
        &self,
        sender: mpsc::Sender<SpotOrder>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            let mut initialized = false;
            let mut ws_stream = match self.connect_to_ws().await {
                Ok(ws_stream) => ws_stream,
                Err(e) => {
                    error!("Failed to establish websocket connection: {:?}", e);
                    continue;
                }
            };

            info!("WebSocket connected");

            ws_stream
                .send(Message::Text(r#"{"type": "connection_init"}"#.into()))
                .await
                .expect("Failed to send init message");

            let mut last_data_time = Instant::now();
            while let Some(message) = ws_stream.next().await {
                if Instant::now().duration_since(last_data_time) > Duration::from_secs(60) {
                    error!("No data messages received for the last 60 seconds, reconnecting...");
                    break;
                }
                match message {
                    Ok(Message::Text(text)) => {
                        let receive_time = SystemTime::now(); // Записываем время получения сообщения

                        if let Ok(response) = serde_json::from_str::<WebSocketResponse>(&text) {
                            match response.r#type.as_str() {
                                "ka" => {
                                    info!("Received keep-alive message.");
                                    last_data_time = Instant::now();
                                    continue;
                                }
                                "connection_ack" => {
                                    if !initialized {
                                        info!("Connection established, subscribing to orders...");
                                        self.subscribe_to_orders(OrderType::Buy, &mut ws_stream)
                                            .await?;
                                        self.subscribe_to_orders(OrderType::Sell, &mut ws_stream)
                                            .await?;
                                        initialized = true;
                                    }
                                }
                                "data" => {
                                    if let Some(payload) = response.payload {
                                        if let Some(orders) = payload.data.active_buy_order {
                                            for order_indexer in orders {
                                                let spot_order =
                                                    SpotOrder::from_indexer(order_indexer)?;
                                                self.log_order_delay(&spot_order, receive_time)
                                                    .await;
                                                sender.send(spot_order).await?;
                                            }
                                        }
                                        if let Some(orders) = payload.data.active_sell_order {
                                            for order_indexer in orders {
                                                let spot_order =
                                                    SpotOrder::from_indexer(order_indexer)?;
                                                self.log_order_delay(&spot_order, receive_time)
                                                    .await;
                                                sender.send(spot_order).await?;
                                            }
                                        }
                                        last_data_time = Instant::now();
                                    }
                                }
                                _ => {}
                            }
                        } else {
                            error!("Failed to deserialize WebSocketResponse: {:?}", text);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("Error in websocket connection: {:?}", e);
                        break;
                    }
                }
            }

            self.unsubscribe_orders(&mut ws_stream, OrderType::Buy)
                .await?;
            self.unsubscribe_orders(&mut ws_stream, OrderType::Sell)
                .await?;
        }
    }

    async fn connect_to_ws(
        &self,
    ) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Box<dyn std::error::Error>> {
        match connect_async(&self.url).await {
            Ok((ws_stream, response)) => {
                info!(
                    "WebSocket handshake has been successfully completed with response: {:?}",
                    response
                );
                Ok(ws_stream)
            }
            Err(e) => {
                error!("Failed to establish websocket connection: {:?}", e);
                Err(Box::new(e))
            }
        }
    }

    async fn subscribe_to_orders(
        &self,
        order_type: OrderType,
        client: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let subscription_query = format_graphql_subscription(order_type);
        let start_msg = serde_json::json!({
            "id": format!("{}", order_type as u8),
            "type": "start",
            "payload": {
                "query": subscription_query
            }
        })
        .to_string();

        client.send(Message::Text(start_msg)).await.map_err(|e| {
            error!("Failed to send subscription: {:?}", e);
            Box::new(e)
        })?;
        Ok(())
    }

    async fn unsubscribe_orders(
        &self,
        client: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        order_type: OrderType,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let stop_msg = serde_json::json!({
            "id": format!("{}", order_type as u8),
            "type": "stop"
        })
        .to_string();
        client.send(Message::Text(stop_msg)).await.map_err(|e| {
            error!("Failed to send unsubscribe message: {:?}", e);
            Box::new(e)
        })?;
        Ok(())
    }

    async fn log_order_delay(&self, order: &SpotOrder, receive_time: SystemTime) {
        // Временная метка блока
        let block_timestamp = order.timestamp;

        // Временная метка получения ордера по вебсокету в секундах с начала Unix-эпохи
        let receive_timestamp = receive_time.duration_since(UNIX_EPOCH).unwrap().as_secs();

        if receive_timestamp >= block_timestamp {
            let delay = receive_timestamp - block_timestamp;
            info!("Order ID: {:?} Delay: {} seconds", order.id, delay);
        } else {
            error!(
                "Receive timestamp is earlier than block timestamp for order ID: {:?}",
                order.id
            );
        }
    }
}
