use ethers_core::types::H256;
use log::debug;
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use pangea_client::{
    futures::StreamExt, provider::FuelProvider, query::Bound, requests::fuel::GetSparkOrderRequest,
    ClientBuilder, Format, WsProvider,
};
use log::{info, error};

use crate::config::settings::Settings;
use crate::error::Error;
use crate::indexer::spot_order::OrderStatus;
use crate::indexer::spot_order::OrderType;
use crate::indexer::spot_order::SpotOrder;
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
    limit_type: Option<String>
}


pub async fn initialize_pangea_indexer(
    settings: Arc<Settings>,
    tasks: &mut Vec<tokio::task::JoinHandle<()>>,
    order_book: Arc<OrderBook>,
) -> Result<(), Error> {
    let ws_task_pangea = tokio::spawn(async move {
        if let Err(e) = start_pangea_indexer((*settings).clone(),order_book).await {
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

    let mut last_processed_block: i64 = 0;
    let contract_start_block: i64 = config.contract.contract_block;
    let contract_h256 = H256::from_str(&config.contract.contract_id).unwrap(); //NTD Remove unwrap
    let threshold = config.websockets.pangea_buy_threshold as u128; 

    let request_all = GetSparkOrderRequest {
        from_block: Bound::Exact(contract_start_block),
        to_block: Bound::Latest,
        market_id__in: HashSet::from([contract_h256]),
        ..Default::default()
    };

    let stream_all = client
        .get_fuel_spark_orders_by_format(request_all, Format::JsonStream, false)
        .await
        .expect("Failed to get fuel spark orders");

    pangea_client::futures::pin_mut!(stream_all);

    info!("Starting to load all historical orders...");
    while let Some(data) = stream_all.next().await {
        match data {
            Ok(data) => {
                let data = String::from_utf8(data).unwrap();
                let order: PangeaOrderEvent = serde_json::from_str(&data).unwrap();
                last_processed_block = order.block_number;
                match order.amount {
                    Some(amount) => {
                        if amount > threshold {
                            handle_order_event(order_book.clone(), order).await;
                        } else {
                            warn!("Skipping order {:?}, amount: {:?} less {:?}", order.order_id, order.amount, threshold);
                        }
                    }
                    None => {
                        warn!("Skipping order {:?}, amount None", order.order_id);
                    } 
                }

            },
            Err(e) => {
                error!("Error in the stream of historical orders: {e}");
                break; 
            }
        }
    }

    info!("Switching to listening for new orders (deltas)");

    loop {
        let request_deltas = GetSparkOrderRequest {
            from_block: Bound::Exact(last_processed_block + 1),
            to_block: Bound::Latest,
            market_id__in: HashSet::from([contract_h256]),
            ..Default::default()
        };

        let stream_deltas = client
            .get_fuel_spark_orders_by_format(request_deltas, Format::JsonStream, true) 
            .await
            .expect("Failed to get fuel spark deltas");

        pangea_client::futures::pin_mut!(stream_deltas);

        while let Some(data) = stream_deltas.next().await {
            match data {
                Ok(data) => {
                    let data = String::from_utf8(data).unwrap();
                    let order: PangeaOrderEvent = serde_json::from_str(&data).unwrap();
                    last_processed_block = order.block_number;
                    match order.amount {
                        Some(amount) => {
                            if amount > threshold {
                                handle_order_event(order_book.clone(), order).await;
                            } else {
                                warn!("Skipping order {:?}, amount: {:?} less {:?}", order.order_id, order.amount, threshold);
                            }
                        }
                        None => {
                            warn!("Skipping order {:?}, amount None", order.order_id);
                        } 
                    }

                },
                Err(e) => {
                    error!("Error in the stream of new orders (deltas): {e}");
                    break; 
                }
            }
        }

        info!("Reconnecting to listen for new deltas...");
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await; 
    }
}

pub async fn handle_order_event(order_book: Arc<OrderBook>, event: PangeaOrderEvent) {
    if let Some(event_type) = event.event_type.as_deref() {
        match event_type {
            "Open" => {
                if let Some(order) = create_new_order_from_event(&event) {
                    order_book.add_order(order);
                    debug!("Added new order with id: {}", event.order_id);
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
                debug!("Removed order with id: {} due to Cancel event", event.order_id);
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



pub fn process_trade(order_book: &OrderBook, order_id: &str, trade_amount: u128, order_type: Option<OrderType>, limit_type: Option<LimitType>) {
    match order_type {
        Some(order_type) => match limit_type {
            Some(limit_type) => {
                match limit_type {
                    LimitType::GTC => {
                        if let Some(mut order) = order_book.get_order(order_id, order_type) {
                            if order.amount > trade_amount {
                                order.amount -= trade_amount;
                                order.status = Some(OrderStatus::PartiallyMatched);
                                order_book.update_order(order.clone());
                                debug!(
                                    "Updated order with id: {} - partially matched, remaining amount: {}",
                                    order_id, order.amount
                                );
                            } else {
                                order.status = Some(OrderStatus::Matched);
                                order_book.remove_order(order_id, Some(order_type));
                                debug!("Removed order with id: {} - fully matched", order_id);
                            }
                        } else {
                            error!("Order with id: {} not found for trade event", order_id);
                        }
                    }
                    _ => {
                        order_book.remove_order(order_id, Some(order_type));
                        debug!("Removed order with id: {} - FOK matched", order_id);
                    }
                }
            }
            None => {
                error!("Limit type is None for order_id: {}. Cannot process trade event.", order_id);
            }
        }
        None => {
            error!("Order type is None for order_id: {}. Cannot process trade event.", order_id);
        }
    }
}

impl PangeaOrderEvent {
    fn order_type_to_enum(&self) -> Option<OrderType> {
        self.order_type.as_deref().and_then(|order_type| match order_type {
            "Buy" => Some(OrderType::Buy),
            "Sell" => Some(OrderType::Sell),
            _ => None,
        })
    }

    fn limit_type_to_enum(&self) -> Option<LimitType> {
        self.limit_type.as_deref().and_then(|limit_type| match limit_type {
            "FOK" => Some(LimitType::FOK),
            "IOC" => Some(LimitType::IOC),
            "GTC" => Some(LimitType::GTC),
            _ => None,
        })
    }
}
