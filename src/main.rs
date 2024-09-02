use config::env::ev;
use error::Error;
use indexer::{envio::WebSocketClient, superchain::start_superchain_indexer};
use middleware::manager::OrderManager;
use tokio::{signal, sync::mpsc};
use url::Url;

pub mod config;
pub mod error;
pub mod indexer;
pub mod middleware;
pub mod subscription;

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();
    env_logger::init();

    let order_manager = OrderManager::new();
    //---------{Envio block
    
    let ws_url_envio = Url::parse(&ev("WEBSOCKET_URL_ENVIO")?)?;
    let websocket_client_envio = WebSocketClient::new(ws_url_envio);

    let (tx, mut rx) = mpsc::channel(100);

    let ws_task_envio = tokio::spawn(async move {
        if let Err(e) = websocket_client_envio.connect(tx).await {
            eprintln!("WebSocket envio error: {}", e);
        }
    });

    let arc_order_manager_envio = order_manager.clone();

    let manager_task_envio= tokio::spawn(async move {
        while let Some(order) = rx.recv().await {
            arc_order_manager_envio.add_order(order).await;
        }
    });
    
    //----------Envio block}

    //---------{Superchain block

    let (tx_superchain, mut rx_superchain) = mpsc::channel(101);

    let ws_task_superchain = tokio::spawn(async move {
        if let Err(e) = start_superchain_indexer(tx_superchain).await {
            eprintln!("Superchain error: {}", e);
        }
    });

    let order_manager_superchain = order_manager.clone();

    let manager_task_superchain = tokio::spawn(async move {
        while let Some(order) = rx_superchain.recv().await {
            order_manager_superchain.add_order(order).await;
        }
    });

    //---------Superchain block}

    let ctrl_c_task = tokio::spawn(async {
        signal::ctrl_c().await.expect("failed to listen for event");
        println!("Ctrl+C received!");
    });

    let _ = tokio::select! {
        //_ = ws_task_envio => { println!("WebSocket task finished"); },
        //_ = manager_task_envio => { println!("Order manager task finished"); },
        _ = ws_task_superchain => { println!("WebSocket superchain task finished"); },
        _ = manager_task_superchain => { println!("Order manager superchain task finished"); },
        _ = ctrl_c_task => { println!("Shutting down..."); },
    };

    println!("Application is shutting down.");

    Ok(())
}
