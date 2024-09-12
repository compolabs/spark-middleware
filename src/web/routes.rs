use std::collections::HashMap;
use std::sync::Arc;

use rocket::serde::json::Json;
use rocket::{get, Route, State};
use rocket_okapi::swagger_ui::SwaggerUIConfig;
use rocket_okapi::{openapi, openapi_get_routes, JsonSchema};
use serde::Serialize;

use crate::indexer::spot_order::{OrderType, SpotOrder};
use crate::middleware::aggregator::Aggregator;
use crate::middleware::manager::OrderManager;

#[derive(Serialize, JsonSchema)]
pub struct OrdersResponse {
    pub orders: Vec<SpotOrder>,
}

#[openapi]
#[get("/orders/<indexer>/buy")]
async fn get_buy_orders(
    indexer: &str,
    managers: &State<HashMap<String, Arc<OrderManager>>>,
) -> Option<Json<OrdersResponse>> {
    if let Some(manager) = managers.get(indexer) {
        let buy_orders = manager.get_all_buy_orders().await;
        return Some(Json(OrdersResponse { orders: buy_orders }));
    }
    None
}

#[openapi]
#[get("/orders/<indexer>/sell")]
async fn get_sell_orders(
    indexer: &str,
    managers: &State<HashMap<String, Arc<OrderManager>>>,
) -> Option<Json<OrdersResponse>> {
    if let Some(manager) = managers.get(indexer) {
        let sell_orders = manager.get_all_sell_orders().await;
        return Some(Json(OrdersResponse {
            orders: sell_orders,
        }));
    }
    None
}

#[openapi]
#[get("/aggregated/orders/buy")]
async fn get_aggregated_buy_orders(aggregator: &State<Arc<Aggregator>>) -> Json<OrdersResponse> {
    let buy_orders = aggregator.get_aggregated_orders(OrderType::Buy).await;
    Json(OrdersResponse { orders: buy_orders })
}

#[openapi]
#[get("/aggregated/orders/sell")]
async fn get_aggregated_sell_orders(aggregator: &State<Arc<Aggregator>>) -> Json<OrdersResponse> {
    let sell_orders = aggregator.get_aggregated_orders(OrderType::Sell).await;
    Json(OrdersResponse {
        orders: sell_orders,
    })
}

pub fn get_routes() -> Vec<Route> {
    openapi_get_routes![
        get_buy_orders,
        get_sell_orders,
        get_aggregated_buy_orders,
        get_aggregated_sell_orders
    ]
}

pub fn get_docs() -> SwaggerUIConfig {
    SwaggerUIConfig {
        url: "/openapi.json".to_string(),
        ..Default::default()
    }
}
