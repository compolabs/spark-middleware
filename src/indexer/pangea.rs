use ethers_core::types::H256;
use log::{error, info, warn};
use pangea_client::Client;
use pangea_client::{
    futures::StreamExt, provider::FuelProvider, query::Bound, requests::fuel::GetSparkOrderRequest,
    ClientBuilder, Format, WsProvider,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

use crate::config::settings::Settings;
use crate::error::Error;
use crate::indexer::spot_order::{OrderStatus, OrderType, SpotOrder};
use crate::storage::order_book::OrderBook;

use super::spot_order::LimitType;

#[derive(Debug, Deserialize, Serialize)]
pub struct PangeaOrderEvent {
    chain: u64,
    block_number: i64,
    block_hash: String,
    transaction_hash: String,
    transaction_index: u64,
    log_index: u64,
    market_id: String,
    order_id: String,
    event_type: Option<String>,
    asset: Option<String>,
    amount: Option<u128>,
    asset_type: Option<String>,
    order_type: Option<String>,
    price: Option<u128>,
    user: Option<String>,
    order_matcher: Option<String>,
    owner: Option<String>,
    limit_type: Option<String>,
}

pub async fn initialize_pangea_indexer(
    settings: Arc<Settings>,
    tasks: &mut Vec<tokio::task::JoinHandle<()>>,
    order_book: Arc<OrderBook>,
) -> Result<(), Error> {
    let ws_task_pangea = tokio::spawn(async move {
        if let Err(e) = start_pangea_indexer((*settings).clone(), order_book).await {
            eprintln!("Pangea error: {}", e);
        }
    });

    tasks.push(ws_task_pangea);
    Ok(())
}

pub async fn start_pangea_indexer(
    config: Settings,
    order_book: Arc<OrderBook>,
) -> Result<(), Error> {
    let username = config.websockets.pangea_username;
    let password = config.websockets.pangea_pass;
    let url = "fuel.beta.pangea.foundation";

    let client = ClientBuilder::default()
        .endpoint(url)
        .credential(&username, &password)
        .build::<WsProvider>()
        .await
        .unwrap();

    info!("Pangea ws client created. Trying to connect");

    let contract_start_block: i64 = config.contract.contract_block;
    let contract_h256 = H256::from_str(&config.contract.contract_id).unwrap();
    info!("Fetching historical orders...");
    let last_processed_block = load_historical_orders(
        &client,
        contract_start_block,
        contract_h256,
        order_book.clone(),
    )
    .await?;

    info!("======");
    info!("Historical orders loaded, starting real time order stream...");
    start_real_time_stream(&client, last_processed_block, contract_h256, order_book).await?;

    Ok(())
}

pub async fn load_historical_orders(
    client: &Client<WsProvider>,
    contract_start_block: i64,
    contract_h256: H256,
    order_book: Arc<OrderBook>,
) -> Result<i64, Error> {
    let mut last_processed_block = contract_start_block;

    let request_all = GetSparkOrderRequest {
        from_block: Bound::Exact(contract_start_block),
        to_block: Bound::Latest,
        market_id__in: HashSet::from([contract_h256]),
        ..Default::default()
    };

    info!("Loading all historical orders...");
    let stream_all = client
        .get_fuel_spark_orders_by_format(request_all, Format::JsonStream, false)
        .await
        .expect("Failed to get fuel spark orders");

    pangea_client::futures::pin_mut!(stream_all);

    while let Some(data) = stream_all.next().await {
        match data {
            Ok(data) => {
                let data = String::from_utf8(data).unwrap();
                let order: PangeaOrderEvent = serde_json::from_str(&data).unwrap();
                last_processed_block = order.block_number;
                handle_order_event(order_book.clone(), order).await;
            }
            Err(e) => {
                error!("Error in the stream of historical orders: {e}");
                break;
            }
        }
    }

    if last_processed_block == contract_start_block {
        warn!("No historical orders found.");
    }

    Ok(last_processed_block)
}

pub async fn start_real_time_stream(
    client: &Client<WsProvider>,
    last_processed_block: i64,
    contract_h256: H256,
    order_book: Arc<OrderBook>,
) -> Result<(), Error> {
    let request_deltas = GetSparkOrderRequest {
        from_block: Bound::Exact(last_processed_block + 1),
        to_block: Bound::Subscribe,
        market_id__in: HashSet::from([contract_h256]),
        ..Default::default()
    };

    loop {
        let stream_deltas = client
            .get_fuel_spark_orders_by_format(request_deltas.clone(), Format::JsonStream, true)
            .await
            .expect("Failed to get fuel spark deltas");

        pangea_client::futures::pin_mut!(stream_deltas);

        while let Ok(Some(data_result)) =
            timeout(Duration::from_secs(20), stream_deltas.next()).await
        {
            match data_result {
                Ok(data) => {
                    let data = String::from_utf8(data).unwrap();
                    let order: PangeaOrderEvent = serde_json::from_str(&data).unwrap();
                    handle_order_event(order_book.clone(), order).await;
                }
                Err(e) => {
                    error!("Error in real-time stream: {e}");
                    break;
                }
            }
        }

        warn!("No data received in the last 20 seconds or stream ended. Reconnecting...");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

pub async fn handle_order_event(order_book: Arc<OrderBook>, event: PangeaOrderEvent) {
    if let Some(event_type) = event.event_type.as_deref() {
        match event_type {
            "Open" => {
                if let Some(order) = create_new_order_from_event(&event) {
                    order_book.add_order(order);
                    info!("Added new order with id: {}", event.order_id);
                }
            }
            "Trade" => {
                if let Some(match_size) = event.amount {
                    let o_type = event.order_type_to_enum();
                    let l_type = event.limit_type_to_enum();
                    process_trade(&order_book, &event.order_id, match_size, o_type, l_type);
                }
            }
            "Cancel" => {
                order_book.remove_order(&event.order_id, event.order_type_to_enum());
                info!(
                    "Removed order with id: {} due to Cancel event",
                    event.order_id
                );
            }
            _ => {
                error!("Unknown event type: {}", event_type);
            }
        }
    }
}

fn create_new_order_from_event(event: &PangeaOrderEvent) -> Option<SpotOrder> {
    if let (Some(price), Some(amount), Some(order_type), Some(user)) = (
        event.price,
        event.amount,
        event.order_type.as_deref(),
        &event.user,
    ) {
        let order_type_enum = match order_type {
            "Buy" => OrderType::Buy,
            "Sell" => OrderType::Sell,
            _ => return None,
        };

        Some(SpotOrder {
            id: event.order_id.clone(),
            user: user.clone(),
            asset: event.asset.clone().unwrap_or_default(),
            amount,
            price,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            order_type: order_type_enum,
            status: Some(OrderStatus::New),
        })
    } else {
        None
    }
}

pub fn process_trade(
    order_book: &OrderBook,
    order_id: &str,
    trade_amount: u128,
    order_type: Option<OrderType>,
    limit_type: Option<LimitType>,
) {
    match order_type {
        Some(order_type) => match limit_type {
            Some(limit_type) => match limit_type {
                LimitType::GTC => {
                    if let Some(mut order) = order_book.get_order(order_id, order_type) {
                        if order.amount > trade_amount {
                            order.amount -= trade_amount;
                            order.status = Some(OrderStatus::PartiallyMatched);
                            order_book.update_order(order.clone());
                        } else {
                            order.status = Some(OrderStatus::Matched);
                            order_book.remove_order(order_id, Some(order_type));
                            info!("Removed order with id: {} - fully matched", order_id);
                        }
                    } else {
                        error!("Order with id: {} not found for trade event", order_id);
                    }
                }
                _ => {
                    order_book.remove_order(order_id, Some(order_type));
                    info!("Removed order with id: {} - FOK matched", order_id);
                }
            },
            None => {
                error!(
                    "Limit type is None for order_id: {}. Cannot process trade event.",
                    order_id
                );
            }
        },
        None => {
            error!(
                "Order type is None for order_id: {}. Cannot process trade event.",
                order_id
            );
        }
    }
}

impl PangeaOrderEvent {
    fn order_type_to_enum(&self) -> Option<OrderType> {
        self.order_type
            .as_deref()
            .and_then(|order_type| match order_type {
                "Buy" => Some(OrderType::Buy),
                "Sell" => Some(OrderType::Sell),
                _ => None,
            })
    }

    fn limit_type_to_enum(&self) -> Option<LimitType> {
        self.limit_type
            .as_deref()
            .and_then(|limit_type| match limit_type {
                "FOK" => Some(LimitType::FOK),
                "IOC" => Some(LimitType::IOC),
                "GTC" => Some(LimitType::GTC),
                _ => None,
            })
    }
}
