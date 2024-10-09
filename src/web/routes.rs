use std::collections::HashMap;
use std::sync::Arc;

use rocket::request::FromParam;
use rocket::serde::json::Json;
use rocket::{get, Route, State};
use rocket_okapi::swagger_ui::SwaggerUIConfig;
use rocket_okapi::{openapi, openapi_get_routes, JsonSchema};
use serde::{Deserialize, Serialize};

use tokio::sync::Mutex;

use crate::indexer::spot_order::{OrderType, SpotOrder};
use crate::middleware::aggregator::Aggregator;
use crate::middleware::manager::OrderManager;
use crate::middleware::order_pool::ShardOrdersDetailedResponse;

#[derive(Serialize, JsonSchema)]
pub struct ShardOrdersCountResponse {
    pub shard_order_counts: Vec<usize>,
}

#[derive(Serialize, JsonSchema)]
pub struct MetricsResponse {
    pub total_matched: u64,
    pub total_remaining: u64,
    pub matched_per_second: f64,
}

#[derive(Serialize, JsonSchema)]
pub struct OrdersCountResponse {
    pub total_orders: usize,
    pub total_buy_orders: usize,
    pub total_sell_orders: usize,
}

#[derive(Serialize, JsonSchema)]
pub struct OrdersResponse {
    pub orders: Vec<SpotOrder>,
}

#[derive(Serialize, Deserialize, Clone, JsonSchema)]
pub enum Indexer {
    Envio,
    Subsquid,
    Pangea,
}

impl Indexer {
    pub fn as_str(&self) -> &'static str {
        match self {
            Indexer::Envio => "envio",
            Indexer::Subsquid => "subsquid",
            Indexer::Pangea => "superchain",
        }
    }

    pub fn all() -> Vec<Indexer> {
        vec![Indexer::Envio, Indexer::Subsquid, Indexer::Pangea]
    }
}

impl<'r> FromParam<'r> for Indexer {
    type Error = &'r str;

    fn from_param(param: &'r str) -> Result<Self, Self::Error> {
        match param {
            "Envio" => Ok(Indexer::Envio),
            "Subsquid" => Ok(Indexer::Subsquid),
            "Pangea" => Ok(Indexer::Pangea),
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
#[get("/orders/count")]
async fn get_orders_count(aggregator: &State<Arc<Aggregator>>) -> Json<OrdersCountResponse> {
    let buy_orders = aggregator.get_all_aggregated_buy_orders().await;
    let sell_orders = aggregator.get_all_aggregated_sell_orders().await;

    let total_orders = buy_orders.len() + sell_orders.len();

    Json(OrdersCountResponse {
        total_orders,
        total_buy_orders: buy_orders.len(),
        total_sell_orders: sell_orders.len(),
    })
}

#[openapi]
#[get("/orders/<indexer>/buy")]
async fn get_buy_orders(
    indexer: Indexer,
    managers: &State<HashMap<String, Arc<OrderManager>>>,
) -> Option<Json<OrdersResponse>> {
    if let Some(manager) = managers.get(indexer.as_str()) {
        let buy_orders = manager.order_pool.get_best_buy_orders();
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
        let sell_orders = manager.order_pool.get_best_sell_orders();
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
        let buy_orders = manager.order_pool.get_best_buy_orders();
        let sell_orders = manager.order_pool.get_best_sell_orders();

        let best_buy = buy_orders
            .iter()
            .max_by_key(|order| order.price)
            .map(|order| order.price);
        let best_sell = sell_orders
            .iter()
            .min_by_key(|order| order.price)
            .map(|order| order.price);

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

    let best_buy = buy_orders
        .iter()
        .max_by_key(|order| order.price)
        .map(|order| order.price);
    let best_sell = sell_orders
        .iter()
        .min_by_key(|order| order.price)
        .map(|order| order.price);

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

#[openapi]
#[get("/orders/<indexer>/shard-details")]
async fn get_shard_order_details(
    indexer: Indexer,
    managers: &State<HashMap<String, Arc<OrderManager>>>,
) -> Option<Json<ShardOrdersDetailedResponse>> {
    if let Some(manager) = managers.get(indexer.as_str()) {
        let shard_details = manager.order_pool.get_detailed_order_info_per_shard();
        let total_buy_orders = shard_details
            .iter()
            .map(|shard| shard.buy_orders_count)
            .sum();
        let total_sell_orders = shard_details
            .iter()
            .map(|shard| shard.sell_orders_count)
            .sum();

        return Some(Json(ShardOrdersDetailedResponse {
            shard_details,
            total_buy_orders,
            total_sell_orders,
        }));
    }
    None
}

#[openapi]
#[get("/aggregated/orders/shard-details")]
async fn get_aggregated_shard_order_details(
    aggregator: &State<Arc<Aggregator>>,
) -> Json<ShardOrdersDetailedResponse> {
    let mut aggregated_shard_details = Vec::new();
    let mut total_buy_orders = 0;
    let mut total_sell_orders = 0;

    for manager in aggregator.order_managers.values() {
        let shard_details = manager.order_pool.get_detailed_order_info_per_shard();
        total_buy_orders += shard_details
            .iter()
            .map(|shard| shard.buy_orders_count)
            .sum::<usize>();
        total_sell_orders += shard_details
            .iter()
            .map(|shard| shard.sell_orders_count)
            .sum::<usize>();
        aggregated_shard_details.extend(shard_details);
    }

    Json(ShardOrdersDetailedResponse {
        shard_details: aggregated_shard_details,
        total_buy_orders,
        total_sell_orders,
    })
}

pub fn get_routes() -> Vec<Route> {
    openapi_get_routes![
        get_buy_orders,
        get_sell_orders,
        get_aggregated_buy_orders,
        get_aggregated_sell_orders,
        get_indexer_spread,
        get_aggregated_spread,
        get_orders_count,
        get_shard_order_details,
        get_aggregated_shard_order_details,
    ]
}

pub fn get_docs() -> SwaggerUIConfig {
    SwaggerUIConfig {
        url: "/openapi.json".to_string(),
        ..Default::default()
    }
}
