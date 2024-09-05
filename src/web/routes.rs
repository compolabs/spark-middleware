
use std::collections::HashMap;
use std::sync::Arc;

use rocket::serde::json::Json;
use rocket::{get, Route, State};
use rocket_okapi::swagger_ui::SwaggerUIConfig;
use rocket_okapi::{openapi, openapi_get_routes, JsonSchema};
use serde::Serialize;

use crate::middleware::manager::OrderManager;

#[derive(Serialize, JsonSchema)]
pub struct OrdersResponse {
    pub orders: Vec<String>,  
}

#[openapi]
#[get("/orders/<indexer>/buy")]
async fn get_buy_orders(
    indexer: &str,
    managers: &State<HashMap<String, Arc<OrderManager>>>,
) -> Option<Json<OrdersResponse>> {
    if let Some(manager) = managers.get(indexer) {
        let buy_orders = manager.get_all_buy_orders().await;
        return Some(Json(OrdersResponse {
            orders: buy_orders.iter().map(|o| o.id.clone()).collect(),
        }));
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
            orders: sell_orders.iter().map(|o| o.id.clone()).collect(),
        }));
    }
    None
}

pub fn get_routes() -> Vec<Route> {
    openapi_get_routes![get_buy_orders, get_sell_orders]
}

pub fn get_docs() -> SwaggerUIConfig {
    SwaggerUIConfig {
        url: "/openapi.json".to_string(),
        ..Default::default()
    }
}

