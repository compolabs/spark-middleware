use config::settings::Settings;
use error::Error;
use futures_util::future::FutureExt;
use futures_util::future::{join_all, select};
use indexer::{
    envio::WebSocketClientEnvio, subsquid::WebSocketClientSubsquid,
    superchain::start_superchain_indexer,
};
use log::info;
use matchers::websocket::MatcherWebSocket;
use middleware::aggregator::Aggregator;
use middleware::manager::OrderManager;
use std::{collections::HashMap, sync::Arc};
use tokio::net::TcpListener;
use tokio::{signal, sync::mpsc};
use url::Url;
use web::server::rocket;

pub mod config;
pub mod error;
pub mod indexer;
pub mod matchers;
pub mod middleware;
pub mod subscription;
pub mod web;

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();
    env_logger::init();

    let settings = Arc::new(Settings::new());
    let mut tasks = vec![];

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
    ));
    tasks.push(rocket_task);

    let matcher_task = tokio::spawn(run_matcher_server(settings.clone(), aggregator));
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
    let websocket_client_envio = WebSocketClientEnvio::new(ws_url_envio);

    let (tx, mut rx) = mpsc::channel(100);
    let ws_task_envio = tokio::spawn(async move {
        if let Err(e) = websocket_client_envio.connect(tx).await {
            eprintln!("WebSocket envio error: {}", e);
        }
    });

    let order_manager_envio = OrderManager::new();
    order_managers.insert("envio".to_string(), order_manager_envio.clone());

    let manager_task_envio = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            order_manager_envio.handle_message(message).await;
        }
    });

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

    let order_manager_subsquid = OrderManager::new();
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

    let order_manager_superchain = OrderManager::new();
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
) {
    let rocket = rocket(order_managers, aggregator, settings);
    let _ = rocket.launch().await;
}

async fn run_matcher_server(settings: Arc<Settings>, aggregator: Arc<Aggregator>) {
    let listener = TcpListener::bind(&settings.matchers.matcher_ws_url)
        .await
        .unwrap(); // NTD unwrap
    let matcher_websocket = Arc::new(MatcherWebSocket::new(settings.clone()));

    info!(
        "Matcher WebSocket server started at {}",
        &settings.matchers.matcher_ws_url
    );

    while let Ok((stream, _)) = listener.accept().await {
        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .expect("Error during WebSocket handshake");

        let matcher_websocket = Arc::clone(&matcher_websocket);
        let aggregator_clone = Arc::clone(&aggregator);

        let (tx, _) = mpsc::channel(100);
        tokio::spawn(async move {
            matcher_websocket
                .handle_connection(ws_stream, tx, aggregator_clone)
                .await;
        });
    }
}
