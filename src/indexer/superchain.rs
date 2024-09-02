use std::collections::HashSet;

use serde::Deserialize;
use superchain_client::{
    core::types::fuel::OrderChangeType, futures::StreamExt, provider::FuelProvider, query::Bound, requests::fuel::{GetFuelBlocksRequest, GetSparkOrderRequest}, Client, ClientBuilder, Format, WsProvider
};
use tokio::{sync::mpsc, time::Instant};
use log::info;

use crate::{config::env::ev, indexer::spot_order::SpotOrder};

pub async fn start_superchain_indexer(_sender: mpsc::Sender<SpotOrder>) -> Result<(), Box<dyn std::error::Error>> {
    let username = ev("SUPERCHAIN_USERNAME")?;
    let password= ev("SUPERCHAIN_PASSWORD")?;
    let _contract_id = ev("CONTRACT_ID_BTC_USDC")?;
    
    let mut h_s_state_types = HashSet::new();
    h_s_state_types.insert(OrderChangeType::Open);
    h_s_state_types.insert(OrderChangeType::Match);


    let client = ClientBuilder::default()
        .credential(username, password)
        .endpoint("fuel.beta.superchain.network")
        .build::<WsProvider>()
        .await?;

    info!("Superchain ws client created. Trying to connect");




    let request = GetSparkOrderRequest {
        from_block: Bound::FromLatest(1000),
        to_block: Bound::None,
        state_type__in: h_s_state_types,
        ..Default::default()
    };

    info!("Superchain ws request: {:?}", request);
    

    let stream = client
        .get_fuel_spark_orders_by_format(request, Format::JsonStream, false)
        .await
        .expect("Failed to get fuel spark orders");
    superchain_client::futures::pin_mut!(stream);


    while let Some(data) = stream.next().await {
        let data = data.unwrap();
        let data_str = String::from_utf8(data).unwrap();
        let superchain_order: SuperchainOrder = serde_json::from_str(&data_str)?;
        let receive_time = Instant::now();
        let a = get_block_timestamp(&client, superchain_order.block_number).await;
        info!("data: {:?}",superchain_order);
    }



    Ok(())
}




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


#[derive(Debug, Deserialize)]
struct BlockData {
    chain: u64,
    block_number: String, 
    hash: String,
    parent_hash: String,
    da_block_number: String, 
    transactions_root: String,
    transactions_count: u64,
    message_receipt_count: u64,
    message_outbox_root: String,
    event_inbox_root: String,
    timestamp: u64, 
}



async fn get_block_timestamp(
    client: &Client<WsProvider>,
    block_number: u64,
) -> Result<u64, Box<dyn std::error::Error>> {
    let request = GetFuelBlocksRequest {
        from_block: Bound::Exact(block_number as i64),
        to_block: Bound::Exact(block_number as i64 + 1), 
        ..Default::default()
    };

    let mut block_stream = client
        .get_fuel_blocks_by_format(request, Format::JsonStream, false)
        .await?;

    if let Some(block_data) = block_stream.next().await {
        let data_str = String::from_utf8(block_data?)?;
        let block_info: BlockData = serde_json::from_str(&data_str)?;         
        info!("=========");
        info!("Block data: {:?}", block_info);
        info!("Block ts: {:?}", block_info.timestamp);
        println!("{b}");
        info!("=========");
        Ok(50)
    } else {
        Err("Failed to retrieve block timestamp".into())
    }
}
