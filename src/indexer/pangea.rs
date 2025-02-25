use async_trait::async_trait;
use ethers_core::types::H256;
use fuels::accounts::provider::Provider;
use fuels::types::Address;
use pangea_client::{
    futures::StreamExt, provider::FuelProvider, query::Bound, requests::fuel::GetSparkOrderRequest,
    ClientBuilder, Format, WsProvider,
};
use pangea_client::{ChainId, Client};
use tokio::task::JoinHandle;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use crate::config::env::ev;
use crate::error::Error;
use crate::indexer::pangea_event_handler::handle_order_event;
use crate::indexer::pangea_event_handler::PangeaOrderEvent;
use crate::storage::order_storage::OrderStorage;
use crate::{
    BUY_ORDERS_TOTAL, ERRORS_TOTAL, ORDER_PROCESSING_DURATION, SELL_ORDERS_TOTAL, SYNC_STATUS,
};

use super::indexer::Indexer;

pub struct PangeaIndexer {
    storage: Arc<OrderStorage>,
}

impl PangeaIndexer {
    pub fn new(storage: Arc<OrderStorage>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl Indexer for PangeaIndexer {
    async fn initialize(&self, tasks: &mut Vec<JoinHandle<()>>) -> Result<(), Error> {
        let storage = Arc::clone(&self.storage);

        let ws_task_pangea = tokio::spawn(async move {
            if let Err(e) = start_pangea_indexer(storage).await {
                error!("Pangea error: {}", e);
            }
        });

        tasks.push(ws_task_pangea);
        Ok(())
    }
}

async fn start_pangea_indexer(storage: Arc<OrderStorage>) -> Result<(), Error> {
    let client = create_pangea_client().await?;

    let contract_start_block: i64 = ev("CONTRACT_START_BLOCK")?.parse()?;
    let contract_h256 = Address::from_str(&ev("CONTRACT_ID")?).unwrap();

    info!("🔄 Starting sync from block {}...", contract_start_block);

    let mut last_processed_block = fetch_historical_data(
        &client,
        &storage,
        contract_start_block,
        contract_h256,
    )
    .await?;

    if last_processed_block == 0 {
        last_processed_block = contract_start_block;
    }

    info!(
        "✅ Sync complete. Switching to realtime indexing from block {}",
        last_processed_block
    );

    listen_for_new_deltas(&client, &storage, last_processed_block, contract_h256).await
}

async fn create_pangea_client() -> Result<Client<WsProvider>, Error> {
    let username = ev("PANGEA_USERNAME")?;
    let password = ev("PANGEA_PASSWORD")?;
    let url = ev("PANGEA_URL")?;

    let client = ClientBuilder::default()
        .endpoint(&url)
        .credential(username, password)
        .build::<WsProvider>()
        .await?;

    info!("✅ Connected to Pangea WebSocket at {}", url);
    Ok(client)
}

async fn get_latest_block(chain_id: ChainId) -> Result<i64, Error> {
    let provider_url = match chain_id {
        ChainId::FUEL => Ok("mainnet.fuel.network"),
        ChainId::FUELTESTNET => Ok("testnet.fuel.network"),
        _ => Err(Error::UnknownChainIdError),
    }?;
    let provider = Provider::connect(provider_url).await?;
    Ok(provider
        .latest_block_height()
        .await
        .map(|height| height as i64)?)
}

async fn fetch_historical_data(
    client: &Client<WsProvider>,
    storage: &Arc<OrderStorage>,
    contract_start_block: i64,
    contract_h256: Address,
) -> Result<i64, Error> {
    let fuel_chain = match ev("CHAIN")?.as_str() {
        "FUEL" => ChainId::FUEL,
        _ => ChainId::FUELTESTNET,
    };
    let mut last_processed_block = contract_start_block;

    let target_latest_block = get_latest_block(fuel_chain).await?;
    info!(
        "📌 Syncing historical data from block {} to {}...",
        contract_start_block, target_latest_block
    );

    let request = GetSparkOrderRequest {
        from_block: Bound::Exact(last_processed_block),
        to_block: Bound::Exact(target_latest_block),
        market_id__in: HashSet::from([contract_h256]),
        chains: HashSet::from([fuel_chain]),
        ..Default::default()
    };

    let stream = client
        .get_fuel_spark_orders_by_format(request, Format::JsonStream, false)
        .await
        .expect("Failed to get fuel spark orders batch");
    pangea_client::futures::pin_mut!(stream);

    while let Some(data) = stream.next().await {
        match data {
            Ok(data) => {
                let data = String::from_utf8(data)?;
                let order: PangeaOrderEvent = serde_json::from_str(&data)?;
                handle_order_event(
                    Arc::clone(&storage.order_book),
                    Arc::clone(&storage.matching_orders),
                    order,
                )
                .await;
            }
            Err(e) => {
                error!("Error in the stream of historical orders: {e}");
                break;
            }
        }
    }

    last_processed_block = target_latest_block;

    info!(
        "✅ Historical sync complete at block {}",
        target_latest_block
    );
    storage.order_book.mark_as_synced();
    SYNC_STATUS.set(1);
    BUY_ORDERS_TOTAL.set(storage.order_book.get_buy_orders().len() as i64);
    SELL_ORDERS_TOTAL.set(storage.order_book.get_sell_orders().len() as i64);
    info!("BUY_ORDERS_TOTAL: {}", BUY_ORDERS_TOTAL.get());
    info!("SELL_ORDERS_TOTAL: {}", SELL_ORDERS_TOTAL.get());
    Ok(last_processed_block)
}

async fn listen_for_new_deltas(
    _client: &Client<WsProvider>,
    storage: &Arc<OrderStorage>,
    mut last_processed_block: i64,
    contract_h256: Address,
) -> Result<(), Error> {
    let mut retry_delay = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    loop {
        match create_pangea_client().await {
            Ok(client) => {
                info!(
                    "🔄 Listening for new orders from block {}",
                    last_processed_block + 1
                );
                let fuel_chain = match ev("CHAIN")?.as_str() {
                    "FUEL" => ChainId::FUEL,
                    _ => ChainId::FUELTESTNET,
                };

                let request_deltas = GetSparkOrderRequest {
                    from_block: Bound::Exact(last_processed_block + 1),
                    to_block: Bound::Subscribe,
                    market_id__in: HashSet::from([contract_h256]),
                    chains: HashSet::from([fuel_chain]),
                    ..Default::default()
                };
                match client
                    .get_fuel_spark_orders_by_format(request_deltas, Format::JsonStream, true)
                    .await
                {
                    Ok(stream_deltas) => {
                        info!("Successfully connected to Pangea stream!");
                        retry_delay = Duration::from_secs(1);

                        pangea_client::futures::pin_mut!(stream_deltas);
                        while let Some(data_result) = stream_deltas.next().await {
                            match data_result {
                                Ok(data) => {
                                    if let Err(e) = process_order_data(
                                        &data,
                                        storage,
                                        &mut last_processed_block,
                                    )
                                    .await
                                    {
                                        warn!("Failed to process order data: {}", e);
                                    }
                                }
                                Err(e) => {
                                    ERRORS_TOTAL.inc();
                                    error!(
                                        "Stream error (block {}): {}",
                                        last_processed_block + 1,
                                        e
                                    );
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to initiate stream: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("Failed to create Pangea client: {}", e);
            }
        }

        sleep(retry_delay).await;
        retry_delay = (retry_delay * 2).min(max_backoff);
    }
}

async fn process_order_data(
    data: &[u8],
    storage: &Arc<OrderStorage>,
    last_processed_block: &mut i64,
) -> Result<(), Error> {
    let timer = ORDER_PROCESSING_DURATION.start_timer();
    let data_str = String::from_utf8(data.to_vec())?;
    let order_event: PangeaOrderEvent = serde_json::from_str(&data_str)?;

    *last_processed_block = order_event.block_number;

    handle_order_event(
        Arc::clone(&storage.order_book),
        Arc::clone(&storage.matching_orders),
        order_event,
    )
    .await;

    timer.observe_duration();
    BUY_ORDERS_TOTAL.set(storage.order_book.get_buy_orders().len() as i64);
    SELL_ORDERS_TOTAL.set(storage.order_book.get_sell_orders().len() as i64);

    debug!(
        "Processed block: {}, BUY_ORDERS_TOTAL: {}, SELL_ORDERS_TOTAL: {}",
        last_processed_block,
        BUY_ORDERS_TOTAL.get(),
        SELL_ORDERS_TOTAL.get()
    );
    Ok(())
}
