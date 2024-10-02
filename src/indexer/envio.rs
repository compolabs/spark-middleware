use std::{sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use tokio::{
    net::TcpStream,
    sync::{mpsc, Mutex},
    time::Instant,
};
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use url::Url;

use crate::{
    config::env::ev,
    error::Error,
    indexer::spot_order::{OrderType, SpotOrder, WebSocketResponseEnvio},
    middleware::manager::OrderManagerMessage,
    subscription::envio::{format_graphql_count_query, format_graphql_pagination_subscription},
};

pub struct WebSocketClientEnvio {
    pub url: Url,
}

impl WebSocketClientEnvio {
    pub fn new(url: Url) -> Self {
        WebSocketClientEnvio { url }
    }

    pub async fn synchronize(
        &self,
        order_type: OrderType,
        sender: mpsc::Sender<OrderManagerMessage>,
    ) -> Result<(), Error> {
        let ws_stream = Arc::new(Mutex::new(self.connect_to_ws().await?));

        info!(
            "Starting synchronization of historical {:?} orders...",
            order_type
        );

        self.synchronize_orders(order_type, ws_stream, sender.clone())
            .await?;

        Ok(())
    }

    pub async fn connect(
        self: Arc<Self>,
        sender: mpsc::Sender<OrderManagerMessage>,
    ) -> Result<(), Error> {
        let ws_stream_sell = Arc::new(Mutex::new(self.connect_to_ws().await?));
        info!("WebSocket connected");

        ws_stream_sell
            .lock()
            .await
            .send(Message::Text(r#"{"type": "connection_init"}"#.into()))
            .await
            .expect("Failed to send init message");

        let ws_stream_buy = Arc::new(Mutex::new(self.connect_to_ws().await?));
        info!("WebSocket connected");

        ws_stream_buy
            .lock()
            .await
            .send(Message::Text(r#"{"type": "connection_init"}"#.into()))
            .await
            .expect("Failed to send init message");

        info!("Starting synchronization of historical sell orders...");
        self.synchronize_orders(OrderType::Sell, ws_stream_sell.clone(), sender.clone())
            .await?;

        info!("Closing WebSocket connection after synchronization...");
        {
            let mut ws_stream_lock = ws_stream_sell.lock().await;
            ws_stream_lock
                .close(None)
                .await
                .expect("Failed to close WebSocket connection properly");
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        info!("Starting synchronization of historical buy orders...");
        self.synchronize_orders(OrderType::Buy, ws_stream_buy.clone(), sender.clone())
            .await?;

        info!("Closing WebSocket connection after synchronization...");
        {
            let mut ws_stream_lock = ws_stream_buy.lock().await;
            ws_stream_lock
                .close(None)
                .await
                .expect("Failed to close WebSocket connection properly");
        }

        let new_ws_stream = Arc::new(Mutex::new(self.connect_to_ws().await?));
        info!("Reconnected WebSocket for subscriptions.");

        {
            let new_ws_stream = new_ws_stream.clone();
            new_ws_stream
                .lock()
                .await
                .send(Message::Text(r#"{"type": "connection_init"}"#.into()))
                .await
                .expect("Failed to send init message after reconnection");
        }

        info!("Synchronization complete, switching to subscription mode...");

        let new_ws_stream_clone = new_ws_stream.clone();
        let buy_subscribe_task = {
            let self_clone = Arc::clone(&self);
            let sender_clone = sender.clone();
            tokio::spawn(async move {
                self_clone
                    .subscribe_to_updates(OrderType::Buy, new_ws_stream_clone, sender_clone)
                    .await
            })
        };

        let new_ws_stream_clone = new_ws_stream.clone();
        let sell_subscribe_task = {
            let self_clone = Arc::clone(&self);
            let sender_clone = sender.clone();
            tokio::spawn(async move {
                self_clone
                    .subscribe_to_updates(OrderType::Sell, new_ws_stream_clone, sender_clone)
                    .await
            })
        };

        let _ = tokio::join!(buy_subscribe_task, sell_subscribe_task);

        Ok(())
    }

    async fn synchronize_orders(
        &self,
        order_type: OrderType,
        client: Arc<Mutex<WebSocketStream<MaybeTlsStream<TcpStream>>>>,
        sender: mpsc::Sender<OrderManagerMessage>,
    ) -> Result<(), Error> {
        let mut offset = 0;
        let limit = ev("FETCH_ORDER_LIMIT")
            .unwrap_or("100".to_string())
            .parse::<u64>()
            .unwrap_or(100);

        let mut total_orders_received = 0;

        loop {
            let subscription_query =
                format_graphql_pagination_subscription(order_type, offset, limit);
            info!(
                "Fetching historical {:?} orders with offset: {}",
                order_type, offset
            );

            let start_msg = serde_json::json!({
                "id": format!("{}", order_type as u8),
                "type": "start",
                "payload": {
                    "query": subscription_query
                }
            })
            .to_string();

            {
                let mut client_lock = client.lock().await;
                client_lock
                    .send(Message::Text(start_msg))
                    .await
                    .map_err(|e| {
                        error!(
                            "Failed to send paginated query for {:?}: {:?}",
                            order_type, e
                        );
                        e
                    })?;
            }

            let response = {
                let mut client_lock = client.lock().await;
                client_lock.next().await
            };

            if let Some(response) = response {
                match response {
                    Ok(Message::Text(text)) => {
                        info!(
                            "Received response for historical {:?} orders with offset {}: {}",
                            order_type, offset, text
                        );
                        if let Ok(parsed_response) =
                            serde_json::from_str::<WebSocketResponseEnvio>(&text)
                        {
                            if let Some(payload) = parsed_response.payload {
                                let orders_to_process = match order_type {
                                    OrderType::Buy => {
                                        payload.data.active_buy_order.unwrap_or_default()
                                    }
                                    OrderType::Sell => {
                                        payload.data.active_sell_order.unwrap_or_default()
                                    }
                                };

                                if orders_to_process.is_empty() {
                                    info!("No more historical orders found for {:?}", order_type);
                                    break;
                                }
                                total_orders_received += orders_to_process.len();

                                for order in orders_to_process {
                                    let order_status = order.status.clone();
                                    match SpotOrder::from_indexer_envio(order) {
                                        Ok(spot_order) => {
                                            if let Some(status) = order_status {
                                                if status == "Active" {
                                                    sender
                                                        .send(OrderManagerMessage::AddOrder(
                                                            spot_order,
                                                        ))
                                                        .await
                                                        .map_err(|_| {
                                                            Error::OrderManagerSendError
                                                        })?;
                                                } else {
                                                    info!(
                                                        "Skipping non-active order with ID: {}",
                                                        spot_order.id
                                                    );
                                                }
                                            } else {
                                                sender
                                                    .send(OrderManagerMessage::AddOrder(spot_order))
                                                    .await
                                                    .map_err(|_| Error::OrderManagerSendError)?;
                                            }
                                        }
                                        Err(e) => error!("Failed to convert order: {:?}", e),
                                    }
                                }
                                offset += limit;
                            }
                        } else {
                            error!("Failed to parse WebSocket response: {:?}", text);
                        }
                    }
                    Ok(_) => warn!(
                        "Received non-text message during synchronization for {:?}",
                        order_type
                    ),
                    Err(e) => {
                        error!("Error during synchronization for {:?}: {:?}", order_type, e);
                        break;
                    }
                }
            }
        }

        info!("==================================================================");
        info!("==================================================================");
        info!("orders {:?}", total_orders_received);
        info!("==================================================================");
        info!("==================================================================");

        Ok(())
    }

    async fn subscribe_to_updates(
        &self,
        order_type: OrderType,
        client: Arc<Mutex<WebSocketStream<MaybeTlsStream<TcpStream>>>>,
        sender: mpsc::Sender<OrderManagerMessage>,
    ) -> Result<(), Error> {
        let subscription_query = format_graphql_pagination_subscription(order_type, 0, 0);

        let start_msg = serde_json::json!({
                "id": format!("{}", order_type as u8),
            "type": "start",
            "payload": {
                "query": subscription_query
            }
        })
        .to_string();

        {
            let mut client_lock = client.lock().await;
            client_lock
                .send(Message::Text(start_msg))
                .await
                .map_err(|e| {
                    error!(
                        "Failed to send subscription query for {:?}: {:?}",
                        order_type, e
                    );
                    e
                })?;
        }

        let mut last_real_data_time = Instant::now();

        loop {
            if Instant::now().duration_since(last_real_data_time)
                > tokio::time::Duration::from_secs(30)
            {
                error!("No real data received for 30 seconds, reconnecting...");
                return Err(Error::EnvioWebsocketConnectionTimeoutError);
            }

            let response = {
                let mut client_lock = client.lock().await;
                client_lock.next().await
            };

            if let Some(response) = response {
                match response {
                    Ok(Message::Text(text)) => {
                        info!("Received WebSocket update for {:?}: {:?}", order_type, text);
                        if let Ok(parsed_response) =
                            serde_json::from_str::<WebSocketResponseEnvio>(&text)
                        {
                            if let Some(payload) = parsed_response.payload {
                                let orders_to_process = match order_type {
                                    OrderType::Buy => {
                                        payload.data.active_buy_order.unwrap_or_default()
                                    }
                                    OrderType::Sell => {
                                        payload.data.active_sell_order.unwrap_or_default()
                                    }
                                };

                                for order in orders_to_process {
                                    let order_status = order.status.clone();
                                    match SpotOrder::from_indexer_envio(order) {
                                        Ok(spot_order) => {
                                            if let Some(status) = order_status {
                                                if status == "Active" {
                                                    sender
                                                        .send(OrderManagerMessage::AddOrder(
                                                            spot_order,
                                                        ))
                                                        .await
                                                        .map_err(|_| {
                                                            Error::OrderManagerSendError
                                                        })?;
                                                } else {
                                                    info!(
                                                        "Skipping non-active order with ID: {}",
                                                        spot_order.id
                                                    );
                                                }
                                            } else {
                                                sender
                                                    .send(OrderManagerMessage::AddOrder(spot_order))
                                                    .await
                                                    .map_err(|_| Error::OrderManagerSendError)?;
                                            }
                                        }
                                        Err(e) => error!("Failed to convert order: {:?}", e),
                                    }
                                }
                                last_real_data_time = Instant::now();
                            }
                        } else {
                            error!("Failed to parse WebSocket response: {:?}", text);
                        }
                    }
                    Ok(_) => warn!("Received non-text message for {:?}", order_type),
                    Err(e) => {
                        error!("WebSocket subscription error for {:?}: {:?}", order_type, e);
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    async fn unsubscribe_orders(
        &self,
        client: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        order_type: OrderType,
    ) -> Result<(), Error> {
        let stop_msg = serde_json::json!({
            "id": format!("{}", order_type as u8),
            "type": "stop"
        })
        .to_string();
        client.send(Message::Text(stop_msg)).await.map_err(|e| {
            error!("Failed to send unsubscribe message: {:?}", e);
            e
        })?;
        Ok(())
    }

    async fn connect_to_ws(&self) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Error> {
        let (ws_stream, _) = connect_async(&self.url).await.map_err(|e| {
            error!("Failed to connect to WebSocket: {:?}", e);
            Error::EnvioWebsocketConnectionError
        })?;
        Ok(ws_stream)
    }
}
