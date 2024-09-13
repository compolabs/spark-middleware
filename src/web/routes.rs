use std::collections::HashMap;
use std::sync::Arc;

use rocket::request::FromParam;
use rocket::serde::json::Json;
use rocket::{get, Route, State};
use rocket_okapi::swagger_ui::SwaggerUIConfig;
use rocket_okapi::{openapi, openapi_get_routes, JsonSchema};
use serde::{Deserialize, Serialize};

use crate::indexer::spot_order::{OrderType, SpotOrder};
use crate::middleware::aggregator::Aggregator;
use crate::middleware::manager::OrderManager;

#[derive(Serialize, JsonSchema)]
pub struct OrdersResponse {
    pub orders: Vec<SpotOrder>,
}

#[derive(Serialize, Deserialize, Clone, JsonSchema)]
pub enum Indexer {
    Envio,
    Subsquid,
    Superchain,
}

impl Indexer {
    pub fn as_str(&self) -> &'static str {
        match self {
            Indexer::Envio => "envio",
            Indexer::Subsquid => "subsquid",
            Indexer::Superchain => "superchain",
        }
    }

    pub fn all() -> Vec<Indexer> {
        vec![Indexer::Envio, Indexer::Subsquid, Indexer::Superchain]
    }
}

impl<'r> FromParam<'r> for Indexer {
    type Error = &'r str;

    fn from_param(param: &'r str) -> Result<Self, Self::Error> {
        match param {
            "Envio" => Ok(Indexer::Envio),
            "Subsquid" => Ok(Indexer::Subsquid),
            "Superchain" => Ok(Indexer::Superchain),
            _ => Err(param),
        }
    }
}

#[derive(Serialize, JsonSchema)]
pub struct SpreadResponse {
    pub buy: Option<u128>,
    pub sell: Option<u128>,
    pub spread: Option<i128>,
}

#[openapi]
#[get("/orders/<indexer>/buy")]
async fn get_buy_orders(
    indexer: Indexer,
    managers: &State<HashMap<String, Arc<OrderManager>>>,
) -> Option<Json<OrdersResponse>> {
    if let Some(manager) = managers.get(indexer.as_str()) {
        let buy_orders = manager.get_all_buy_orders().await;
        return Some(Json(OrdersResponse { orders: buy_orders }));
    }
    None
}

#[openapi]
#[get("/orders/<indexer>/sell")]
async fn get_sell_orders(
    indexer: Indexer,
    managers: &State<HashMap<String, Arc<OrderManager>>>,
) -> Option<Json<OrdersResponse>> {
    if let Some(manager) = managers.get(indexer.as_str()) {
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

#[openapi]
#[get("/spread/<indexer>")]
async fn get_indexer_spread(
    indexer: Indexer,
    managers: &State<HashMap<String, Arc<OrderManager>>>,
) -> Option<Json<SpreadResponse>> {
    if let Some(manager) = managers.get(indexer.as_str()) {
        let buy_orders = manager.get_all_buy_orders().await;
        let sell_orders = manager.get_all_sell_orders().await;

        let best_buy = buy_orders.iter().max_by_key(|order| order.price).map(|order| order.price);
        let best_sell = sell_orders.iter().min_by_key(|order| order.price).map(|order| order.price);

        let spread = if let (Some(buy), Some(sell)) = (best_buy, best_sell) {
            Some(sell as i128 - buy as i128)
        } else {
            None
        };

        return Some(Json(SpreadResponse {
            buy: best_buy,
            sell: best_sell,
            spread,
        }));
    }
    None
}

#[openapi]
#[get("/aggregated/spread")]
async fn get_aggregated_spread(aggregator: &State<Arc<Aggregator>>) -> Json<SpreadResponse> {
    let buy_orders = aggregator
        .get_aggregated_orders(crate::indexer::spot_order::OrderType::Buy)
        .await;
    let sell_orders = aggregator
        .get_aggregated_orders(crate::indexer::spot_order::OrderType::Sell)
        .await;

    let best_buy = buy_orders.iter().max_by_key(|order| order.price).map(|order| order.price);
    let best_sell = sell_orders.iter().min_by_key(|order| order.price).map(|order| order.price);

    let spread = if let (Some(buy), Some(sell)) = (best_buy, best_sell) {
        Some(sell as i128 - buy as i128)
    } else {
        None
    };

    Json(SpreadResponse {
        buy: best_buy,
        sell: best_sell,
        spread,
    })
}

pub fn get_routes() -> Vec<Route> {
    openapi_get_routes![
        get_buy_orders,
        get_sell_orders,
        get_aggregated_buy_orders,
        get_aggregated_sell_orders,
        get_indexer_spread,
        get_aggregated_spread
    ]
}

pub fn get_docs() -> SwaggerUIConfig {
    SwaggerUIConfig {
        url: "/openapi.json".to_string(),
        ..Default::default()
    }
}
