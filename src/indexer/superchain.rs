use log::{info, error};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use superchain_client::{
    core::types::fuel::OrderChangeType,
    futures::StreamExt,
    provider::FuelProvider,
    query::Bound,
    requests::fuel::{GetFuelBlocksRequest, GetSparkOrderRequest},
    Client, ClientBuilder, Format, WsProvider,
};
use tokio::sync::{mpsc, Mutex};

use crate::config::settings::Settings;
use crate::error::Error;
use crate::indexer::spot_order::OrderType;
use crate::middleware::manager::OrderManagerMessage;
use crate::{config::env::ev, indexer::spot_order::SpotOrder};

#[derive(Debug, Deserialize)]
struct SuperchainOrder {
    chain: u64,
    block_number: u64,
    block_hash: String,
    transaction_hash: String,
    transaction_index: u64,
    log_index: u64,
    market_id: String,
    order_id: String,
    state_type: String,
    asset: String,
    amount: Option<u128>,
    asset_type: Option<String>,
    order_type: Option<String>,
    price: Option<u128>,
    user: Option<String>,
    order_matcher: Option<String>,
    owner: Option<String>,
    counterparty: Option<String>,
    match_size: Option<u128>,
    match_price: Option<u128>,
}

pub async fn start_superchain_indexer(
    sender: mpsc::Sender<OrderManagerMessage>,
    config: Settings
) -> Result<(), Error> {
    let username = config.websockets.superchain_username;
    let password = config.websockets.superchain_pass;

    let client = ClientBuilder::default()
        .credential(&username, &password)
        .endpoint("fuel.beta.superchain.network")
        .build::<WsProvider>()
        .await?;

    info!("Superchain ws client created. Trying to connect");

    let request = GetSparkOrderRequest {
        from_block: Bound::FromLatest(10000),
        to_block: Bound::None,
        ..Default::default()
    };

    let stream = client
        .get_fuel_spark_orders_by_format(request, Format::JsonStream, false)
        .await
        .expect("Failed to get fuel spark orders");
    superchain_client::futures::pin_mut!(stream);

    while let Some(data) = stream.next().await {
        match data {
            Ok(valid_data) => {
                let data_str = String::from_utf8(valid_data).unwrap();
                let superchain_order: SuperchainOrder = serde_json::from_str(&data_str)?;

                match superchain_order.state_type.as_str() {
                    "Open" => {
                        if let (Some(amount), Some(price), Some(order_type), Some(user)) = (
                            superchain_order.amount,
                            superchain_order.price,
                            superchain_order.order_type.as_ref(),
                            superchain_order.user.as_ref(),
                        ) {
                            let order_type_enum = match order_type.as_str() {
                                "Buy" => OrderType::Buy,
                                "Sell" => OrderType::Sell,
                                _ => {
                                    error!("Unknown order type: {}", order_type);
                                    continue;
                                }
                            };

                            let spot_order = SpotOrder {
                                id: superchain_order.order_id.clone(),
                                user: user.clone(),
                                asset: superchain_order.asset.clone(),
                                amount,
                                price,
                                timestamp: SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs(),
                                order_type: order_type_enum,
                            };

                            info!("Sending Open Order to manager: {:?}", spot_order);
                            if let Err(e) = sender.send(OrderManagerMessage::AddOrder(spot_order)).await {
                                error!("Failed to send order to manager: {:?}", e);
                            }
                        }
                    }
                    "Match" | "Cancel" => {
                        let order_id = superchain_order.order_id.clone();
                        let price = superchain_order.price.unwrap_or_default(); 

                        if let Some(order_type_str) = superchain_order.order_type.as_deref() {
                            let order_type_enum = match order_type_str {
                                "Buy" => OrderType::Buy,
                                "Sell" => OrderType::Sell,
                                _ => {
                                    error!("Unknown order type for removing order: {:?}", order_type_str);
                                    continue;
                                }
                            };

                            info!("Removing Order from manager: {:?}, with order_type: {:?}", order_id, order_type_enum);

                            let remove_message = OrderManagerMessage::RemoveOrder {
                                order_id,
                                price,
                                order_type: order_type_enum,
                            };

                            if let Err(e) = sender.send(remove_message).await {
                                error!("Failed to send remove order message to manager: {:?}", e);
                            }
                        } else {
                            info!("Removing Order from manager without known order_type: {:?}", order_id);

                            let remove_buy_message = OrderManagerMessage::RemoveOrder {
                                order_id: order_id.clone(),
                                price,
                                order_type: OrderType::Buy,
                            };

                            let remove_sell_message = OrderManagerMessage::RemoveOrder {
                                order_id,
                                price,
                                order_type: OrderType::Sell,
                            };

                            if let Err(e) = sender.send(remove_buy_message).await {
                                error!("Failed to send remove buy order message to manager: {:?}", e);
                            }

                            if let Err(e) = sender.send(remove_sell_message).await {
                                error!("Failed to send remove sell order message to manager: {:?}", e);
                            }
                        }
                    }
                    _ => {
                        error!("Unknown state_type: {}", superchain_order.state_type);
                    }
                }
            }
            Err(e) => {
                error!("Error {:?}", e);
            }
        }
    }

    Ok(())
}
