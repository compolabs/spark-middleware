use log::{error, info};
use serde::Deserialize;
use std::sync::Arc;
use pangea_client::{
    futures::StreamExt, provider::FuelProvider, query::Bound, requests::fuel::GetSparkOrderRequest,
    ClientBuilder, Format, WsProvider,
};

use crate::config::settings::Settings;
use crate::error::Error;
use crate::indexer::spot_order::OrderStatus;
use crate::indexer::spot_order::OrderType;
use crate::indexer::spot_order::SpotOrder;
use crate::storage::order_book::OrderBook;

#[derive(Debug, Deserialize)]
struct PangeaOrder {
    chain: u64,
    block_number: i64,
    block_hash: String,
    transaction_hash: String,
    transaction_index: u64,
    log_index: u64,
    market_id: String,
    order_id: String,
    #[serde(rename = "event_type")]
    state_type: Option<String>, 
    asset: Option<String>, 
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

    
    let request_all = GetSparkOrderRequest {
        from_block: Bound::FromLatest(10000),
        to_block: Bound::Latest,
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
                let order: PangeaOrder = serde_json::from_str(&data).unwrap();
                last_processed_block = order.block_number;
//                info!("{:?}",order.state_type);
                info!("order {:?} {:?}", order.order_id, order.state_type); 
                info!("======");
                info!("{:?}",order);
                info!("======");

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
                    let order: PangeaOrder = serde_json::from_str(&data).unwrap();
                    last_processed_block = order.block_number;
                    info!("order {:?} {:?}", order.order_id, order.state_type); 
                    info!("======");
                    info!("{:?}",order);
                    info!("======");
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
