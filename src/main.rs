use config::settings::Settings;
use error::Error;
use futures_util::future::FutureExt;
use futures_util::future::{join_all, select};
use indexer::{
    envio::WebSocketClientEnvio, subsquid::WebSocketClientSubsquid,
    superchain::start_superchain_indexer,
};
use log::info;
use matchers::manager::MatcherManager;
use matchers::websocket::MatcherWebSocket;
use metrics::types::OrderMetrics;
use middleware::aggregator::Aggregator;
use middleware::manager::OrderManager;
use middleware::order_pool::{generate_price_time_ranges, ShardedOrderPool};
use std::{collections::HashMap, sync::Arc};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::{signal, sync::mpsc};
use url::Url;
use web::server::rocket;

pub mod config;
pub mod error;
pub mod indexer;
pub mod matchers;
pub mod metrics;
pub mod middleware;
pub mod subscription;
pub mod web;

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();
    env_logger::init();

    let settings = Arc::new(Settings::new());
    let mut tasks = vec![];

    let matcher_manager = Arc::new(Mutex::new(MatcherManager::new()));

    let mut order_managers: HashMap<String, Arc<OrderManager>> = HashMap::new();

    let order_metrics = Arc::new(Mutex::new(OrderMetrics::load_from_file()));

    let mut order_managers: HashMap<String, Arc<OrderManager>> = HashMap::new();

    if settings
        .settings
        .active_indexers
        .contains(&"envio".to_string())
    {
        initialize_envio_indexer(settings.clone(), &mut tasks, &mut order_managers).await?;
    }

    if settings
        .settings
        .active_indexers
        .contains(&"subsquid".to_string())
    {
        initialize_subsquid_indexer(settings.clone(), &mut tasks, &mut order_managers).await?;
    }

    if settings
        .settings
        .active_indexers
        .contains(&"superchain".to_string())
    {
        initialize_superchain_indexer(settings.clone(), &mut tasks, &mut order_managers).await?;
    }

    let aggregator = Aggregator::new(
        order_managers.clone(),
        settings.settings.active_indexers.clone(),
    );

    let rocket_task = tokio::spawn(run_rocket_server(
        order_managers.clone(),
        Arc::clone(&aggregator),
        Arc::clone(&settings),
        Arc::clone(&order_metrics),
    ));
    tasks.push(rocket_task);

    let envio_manager = order_managers.get("envio").expect("Envio manager not found");

    let matcher_task = tokio::spawn(run_matcher_server(
        settings.clone(),
        Arc::clone(&envio_manager.order_pool),
        matcher_manager.clone(),
        order_metrics,
    ));
    tasks.push(matcher_task);

    let ctrl_c_task = tokio::spawn(async {
        signal::ctrl_c().await.expect("failed to listen for event");
        println!("Ctrl+C received!");
    });
    tasks.push(ctrl_c_task);

    let shutdown_signal = signal::ctrl_c().map(|_| {
        println!("Shutting down gracefully...");
    });

    select(join_all(tasks).boxed(), shutdown_signal.boxed()).await;

    println!("Application is shutting down.");
    Ok(())
}

async fn initialize_envio_indexer(
    settings: Arc<Settings>,
    tasks: &mut Vec<tokio::task::JoinHandle<()>>,
    order_managers: &mut HashMap<String, Arc<OrderManager>>,
) -> Result<(), Error> {
    let ws_url_envio = Url::parse(&settings.websockets.envio_url)?;
    let websocket_client_envio = Arc::new(WebSocketClientEnvio::new(ws_url_envio));

    let (tx, mut rx) = mpsc::channel(100);

    let ws_task_envio = {
        let websocket_client_envio = Arc::clone(&websocket_client_envio);
        tokio::spawn(async move {
            if let Err(e) = websocket_client_envio.connect(tx).await {
                eprintln!("WebSocket envio error: {}", e);
            }
        })
    };

    let price_time_ranges = generate_price_time_ranges(); 
    let order_manager_envio = OrderManager::new(price_time_ranges);
    order_managers.insert("envio".to_string(), order_manager_envio.clone());

    let manager_task_envio = {
        let order_manager_envio = Arc::clone(&order_manager_envio);
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                order_manager_envio.handle_message(message).await;
            }
        })
    };

    tasks.push(ws_task_envio);
    tasks.push(manager_task_envio);

    Ok(())
}

async fn initialize_subsquid_indexer(
    settings: Arc<Settings>,
    tasks: &mut Vec<tokio::task::JoinHandle<()>>,
    order_managers: &mut HashMap<String, Arc<OrderManager>>,
) -> Result<(), Error> {
    let ws_url_subsquid = Url::parse(&settings.websockets.subsquid_url)?;
    let websocket_client_subsquid = WebSocketClientSubsquid::new(ws_url_subsquid);

    let (tx, mut rx) = mpsc::channel(102);
    let ws_task_subsquid = tokio::spawn(async move {
        if let Err(e) = websocket_client_subsquid.connect(tx).await {
            eprintln!("WebSocket subsquid error: {}", e);
        }
    });

    let price_time_ranges = generate_price_time_ranges(); 
    let order_manager_subsquid = OrderManager::new(price_time_ranges);
    order_managers.insert("subsquid".to_string(), order_manager_subsquid.clone());

    let manager_task_subsquid = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            order_manager_subsquid.handle_message(message).await;
        }
    });

    tasks.push(ws_task_subsquid);
    tasks.push(manager_task_subsquid);
    Ok(())
}

async fn initialize_superchain_indexer(
    settings: Arc<Settings>,
    tasks: &mut Vec<tokio::task::JoinHandle<()>>,
    order_managers: &mut HashMap<String, Arc<OrderManager>>,
) -> Result<(), Error> {
    let (tx_superchain, mut rx_superchain) = mpsc::channel(101);
    let ws_task_superchain = tokio::spawn(async move {
        if let Err(e) = start_superchain_indexer(tx_superchain, (*settings).clone()).await {
            eprintln!("Superchain error: {}", e);
        }
    });

    let price_time_ranges = generate_price_time_ranges(); 
    let order_manager_superchain = OrderManager::new(price_time_ranges);
    order_managers.insert("superchain".to_string(), order_manager_superchain.clone());

    let manager_task_superchain = tokio::spawn(async move {
        while let Some(message) = rx_superchain.recv().await {
            order_manager_superchain.handle_message(message).await;
        }
    });

    tasks.push(ws_task_superchain);
    tasks.push(manager_task_superchain);
    Ok(())
}

async fn run_rocket_server(
    order_managers: HashMap<String, Arc<OrderManager>>,
    aggregator: Arc<Aggregator>,
    settings: Arc<Settings>,
    metrics: Arc<Mutex<OrderMetrics>>,
) {
    let rocket = rocket(order_managers, aggregator, settings, metrics);
    let _ = rocket.launch().await;
}

async fn run_matcher_server(
    settings: Arc<Settings>,
    order_pool: Arc<ShardedOrderPool>, 
    matcher_manager: Arc<Mutex<MatcherManager>>,
    metrics: Arc<Mutex<OrderMetrics>>,
) {
    let listener = TcpListener::bind(&settings.matchers.matcher_ws_url)
        .await
        .unwrap(); 
    let matcher_websocket = Arc::new(MatcherWebSocket::new(settings.clone(), metrics.clone()));

    info!(
        "Matcher WebSocket server started at {}",
        &settings.matchers.matcher_ws_url
    );

    while let Ok((stream, _)) = listener.accept().await {
        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .expect("Error during WebSocket handshake");

        let matcher_websocket = Arc::clone(&matcher_websocket);
        let order_pool_clone = Arc::clone(&order_pool); 
        let matcher_manager_clone = Arc::clone(&matcher_manager);

        let (tx, _) = mpsc::channel(100);
        tokio::spawn(async move {
            matcher_websocket
                .handle_connection(ws_stream, tx, order_pool_clone, matcher_manager_clone)
                .await;
        });
    }
}
