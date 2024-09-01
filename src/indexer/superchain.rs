use superchain_client::{
    futures::StreamExt,
    provider::FuelProvider, query::Bound, requests::fuel::GetSparkOrderRequest, ClientBuilder,
    Format, WsProvider,
};
use tokio::sync::mpsc;
use crate::{config::env::ev, indexer::spot_order::SpotOrder};

pub async fn start_superchain_indexer(_sender: mpsc::Sender<SpotOrder>) -> Result<(), Box<dyn std::error::Error>> {
    let username = ev("SUPERCHAIN_USERNAME")?;
    let password= ev("SUPERCHAIN_PASSWORD")?;
    let client = ClientBuilder::default()
        .credential(username, password)
        .endpoint("fuel.beta.superchain.network")
        .build::<WsProvider>()
        .await?;

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
        let data = data.unwrap();
        let data = String::from_utf8(data).unwrap();
        println!("data: {data}");
    }

    Ok(())
}
