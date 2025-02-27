use async_trait::async_trait;
use chrono::NaiveDateTime;
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;
use tokio::{net::TcpStream, task::JoinHandle};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, protocol::Message},
    MaybeTlsStream, WebSocketStream,
};
use tracing::{error, info, warn};
use url::Url;

use crate::error::Error;
use crate::indexer::envio_event_handler::handle_envio_event;
use crate::indexer::spot_order::{OrderStatus, OrderType, SpotOrder};
use crate::storage::order_storage::OrderStorage;
use crate::{
    config::env::ev, indexer::envio_event_handler::parse_envio_event,
    storage::matching_orders::MatchingOrders,
};

use super::Indexer;

pub struct EnvioIndexer {
    storage: Arc<OrderStorage>,
    client: Client,
}

impl EnvioIndexer {
    pub fn new(storage: Arc<OrderStorage>) -> Self {
        Self {
            storage,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl Indexer for EnvioIndexer {
    async fn initialize(&self, tasks: &mut Vec<JoinHandle<()>>) -> Result<(), Error> {
        let storage = Arc::clone(&self.storage);
        let client = self.client.clone();

        // 1. Фетчим исторические данные в отдельной задаче, но ждём её завершения перед подпиской
        let historical_task = tokio::spawn(async move {
            if let Err(e) = fetch_historical_data(&client, &storage).await {
                error!("Envio historical data fetch error: {}", e);
            }
        });

        // Ждём завершения исторического фетча перед подпиской на real-time события
        if let Err(e) = historical_task.await {
            error!("Historical data fetch task panicked: {:?}", e);
        }

        let storage_clone = Arc::clone(&self.storage);
        let client_clone = self.client.clone();

        // 2. После завершения фетча истории запускаем подписку на real-time события
        let realtime_task = tokio::spawn(async move {
            if let Err(e) = listen_for_envio_events(&client_clone, &storage_clone).await {
                error!("Envio real-time event listener error: {}", e);
            }
        });
        tasks.push(realtime_task);

        Ok(())
    }
}

async fn fetch_historical_data(client: &Client, storage: &Arc<OrderStorage>) -> Result<(), Error> {
    let graphql_url = ev("ENVIO_HTTP_URL")?;
    let market = ev("CONTRACT_ID")?;

    let buy_query = format!(
        r#"{{ "query": "query ActiveBuyQuery {{ ActiveBuyOrder(where: {{market: {{_eq: \"{}\"}}}}) {{ id price amount user orderType limitType asset status timestamp }} }}" }}"#,
        market
    );

    let sell_query = format!(
        r#"{{ "query": "query ActiveSellQuery {{ ActiveSellOrder(where: {{market: {{_eq: \"{}\"}}}}) {{ id price amount user orderType limitType asset status timestamp }} }}" }}"#,
        market
    );

    let buy_orders = fetch_orders(client, &graphql_url, &buy_query).await?;
    let sell_orders = fetch_orders(client, &graphql_url, &sell_query).await?;

    for order in buy_orders {
        storage.order_book.add_order(order);
    }
    for order in sell_orders {
        storage.order_book.add_order(order);
    }

    info!("✅ Envio historical sync complete");
    Ok(())
}

async fn fetch_orders(client: &Client, url: &str, query: &str) -> Result<Vec<SpotOrder>, Error> {
    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(query.to_string())
        .send()
        .await
        .unwrap();

    let json: Value = response.json().await.unwrap();
    let orders = json["data"]["ActiveBuyOrder"]
        .as_array()
        .or_else(|| json["data"]["ActiveSellOrder"].as_array())
        .ok_or_else(|| Error::Other("Invalid response format".into()))?;

    let mut result = Vec::new();
    for order in orders {
        let id = order["id"].as_str().unwrap_or_default().to_string();
        let price = order["price"]
            .as_str()
            .unwrap_or("0")
            .parse::<u128>()
            .unwrap_or(0);
        let amount = order["amount"]
            .as_str()
            .unwrap_or("0")
            .parse::<u128>()
            .unwrap_or(0);
        let user = order["user"].as_str().unwrap_or_default().to_string();
        let asset = order["asset"].as_str().unwrap_or_default().to_string();
        let order_type = match order["orderType"].as_str() {
            Some("Buy") => OrderType::Buy,
            Some("Sell") => OrderType::Sell,
            _ => continue,
        };
        let timestamp = order["timestamp"]
            .as_str()
            .and_then(|ts| NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.3fZ").ok())
            .map(|dt| dt.and_utc().timestamp() as u64)
            .unwrap_or(0);

        result.push(SpotOrder {
            id,
            user,
            asset,
            amount,
            price,
            timestamp,
            order_type,
            limit_type: None,
            status: Some(OrderStatus::New),
        });
    }

    Ok(result)
}

/// Формируем тело subscription'а для OpenOrderEvent
fn format_open_order_subscription(market: &str) -> String {
    serde_json::json!({
        "id": "1",
        "type": "start",
        "payload": {
            "query": format!(
                r#"
                  subscription OpenOrderEvent {{
                    OpenOrderEvent(where: {{market: {{_eq: "{}"}}}}) {{
                      amount
                      asset
                      baseAmount
                      db_write_timestamp
                      id
                      limitType
                      market
                      orderId
                      orderType
                      price
                      quoteAmount
                      timestamp
                      txId
                      user
                    }}
                  }}
                "#,
                market
            )
        }
    })
    .to_string()
}

/// Формируем тело subscription'а для CancelOrderEvent
fn format_cancel_order_subscription(market: &str) -> String {
    serde_json::json!({
        "id": "2",
        "type": "start",
        "payload": {
            "query": format!(
                r#"
                  subscription CancelOrderEvent {{
                    CancelOrderEvent(where: {{market: {{_eq: "{}"}}}}) {{
                      baseAmount
                      db_write_timestamp
                      id
                      market
                      orderId
                      quoteAmount
                      timestamp
                      user
                      txId
                    }}
                  }}
                "#,
                market
            )
        }
    })
    .to_string()
}

/// Формируем тело subscription'а для TradeOrderEvent
fn format_trade_order_subscription(market: &str) -> String {
    serde_json::json!({
        "id": "3",
        "type": "start",
        "payload": {
            "query": format!(
                r#"
                  subscription TradeOrderEvent {{
                    TradeOrderEvent(where: {{market: {{_eq: "{}"}}}}) {{
                      buyOrderId
                      buyer
                      buyerQuoteAmount
                      buyerBaseAmount
                      id
                      db_write_timestamp
                      market
                      sellOrderId
                      seller
                      sellerBaseAmount
                      sellerIsMaker
                      sellerQuoteAmount
                      timestamp
                      tradePrice
                      tradeSize
                      txId
                    }}
                  }}
                "#,
                market
            )
        }
    })
    .to_string()
}

pub async fn listen_for_envio_events(
    _client: &reqwest::Client,
    storage: &Arc<OrderStorage>,
) -> Result<(), Error> {
    let ws_url = ev("ENVIO_WS_URL")?;
    let market = ev("CONTRACT_ID")?;

    let url =
        Url::parse(&ws_url).map_err(|e| Error::Other(format!("Invalid ENVIO_WS_URL: {}", e)))?;

    let mut ws_stream = subscribe_to_envio_events(url, &market).await?;
    let order_book = Arc::clone(&storage.order_book);
    let matching_orders = Arc::new(MatchingOrders::new());

    info!("✅ Subscribed to Envio events for market: {market}");

    while let Some(message_result) = ws_stream.next().await {
        match message_result {
            Ok(msg) => match msg {
                Message::Text(text) => match serde_json::from_str::<Value>(&text) {
                    Ok(json_val) => {
                        let msg_type = json_val["type"].as_str().unwrap_or_default();
                        match msg_type {
                            "connection_ack" => {
                                info!("✅ Received connection_ack");
                            }
                            "ka" => {
                                info!("♻️  Keep-alive (ka) message received");
                            }
                            "data" => {
                                if let Some(payload) =
                                    json_val.get("payload").and_then(|p| p.get("data"))
                                {
                                    if let Some(open_orders) =
                                        payload.get("OpenOrderEvent").and_then(|v| v.as_array())
                                    {
                                        for order_value in open_orders {
                                            if let Some(event) =
                                                parse_envio_event(order_value, "OpenOrderEvent")
                                            {
                                                handle_envio_event(
                                                    order_book.clone(),
                                                    matching_orders.clone(),
                                                    event,
                                                )
                                                .await;
                                            }
                                        }
                                    }
                                    if let Some(cancel_orders) =
                                        payload.get("CancelOrderEvent").and_then(|v| v.as_array())
                                    {
                                        for order_value in cancel_orders {
                                            if let Some(event) =
                                                parse_envio_event(order_value, "CancelOrderEvent")
                                            {
                                                handle_envio_event(
                                                    order_book.clone(),
                                                    matching_orders.clone(),
                                                    event,
                                                )
                                                .await;
                                            }
                                        }
                                    }
                                    if let Some(trade_orders) =
                                        payload.get("TradeOrderEvent").and_then(|v| v.as_array())
                                    {
                                        for order_value in trade_orders {
                                            if let Some(event) =
                                                parse_envio_event(order_value, "TradeOrderEvent")
                                            {
                                                handle_envio_event(
                                                    order_book.clone(),
                                                    matching_orders.clone(),
                                                    event,
                                                )
                                                .await;
                                            }
                                        }
                                    }
                                }
                            }
                            other => {
                                info!("Received unhandled message type: {}", other);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Could not parse JSON from text: {}, error: {:?}", text, e);
                    }
                },
                Message::Binary(_) => {
                    info!("Received binary message (ignored)");
                }
                Message::Close(c) => {
                    info!("Received close message: {:?}", c);
                    break;
                }
                Message::Ping(p) => {
                    info!("Received ping: {:?}", p);
                }
                Message::Pong(p) => {
                    info!("Received pong: {:?}", p);
                }
                Message::Frame(_) => todo!(),
            },
            Err(e) => {
                error!("WebSocket error: {:?}", e);
                break;
            }
        }
    }

    info!("listen_for_envio_events finished");
    Ok(())
}

pub async fn subscribe_to_envio_events(
    graphql_url: Url,
    market: &str,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Error> {
    // Формируем клиентский запрос
    let mut request = graphql_url.as_str().into_client_request()?;
    // Говорим, что протокол у нас GraphQL
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", "graphql-ws".parse().unwrap());

    // Устанавливаем WebSocket-соединение
    let (mut ws_stream, _) = connect_async(request).await?;

    // Инициируем соединение
    ws_stream
        .send(Message::Text(r#"{"type": "connection_init"}"#.into()))
        .await?;

    // Подписка на OpenOrderEvent
    ws_stream
        .send(Message::Text(format_open_order_subscription(market).into()))
        .await?;

    // Подписка на CancelOrderEvent
    ws_stream
        .send(Message::Text(
            format_cancel_order_subscription(market).into(),
        ))
        .await?;

    // Подписка на TradeOrderEvent
    ws_stream
        .send(Message::Text(
            format_trade_order_subscription(market).into(),
        ))
        .await?;

    info!("Subscribed to Envio (Open/Cancel/Trade) on {}", graphql_url);

    // Возвращаем поток, чтобы можно было считывать события снаружи
    Ok(ws_stream)
}
