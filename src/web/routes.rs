use std::collections::HashMap;
use std::sync::Arc;

use rocket::request::FromParam;
use rocket::serde::json::Json;
use rocket::{get, Route, State};
use rocket_okapi::swagger_ui::SwaggerUIConfig;
use rocket_okapi::{openapi, openapi_get_routes, JsonSchema};
use serde::{Deserialize, Serialize};

use async_graphql_rocket::{GraphQLQuery, GraphQLRequest, GraphQLResponse};
use tokio::sync::Mutex;

use crate::indexer::spot_order::{OrderType, SpotOrder};
use crate::metrics::types::OrderMetrics;
use crate::middleware::aggregator::Aggregator;
use crate::middleware::manager::OrderManager;

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
#[get("/metrics")]
async fn get_metrics(metrics: &State<Arc<Mutex<OrderMetrics>>>) -> Json<MetricsResponse> {
    let metrics = metrics.lock().await;
    Json(MetricsResponse {
        total_matched: metrics.total_matched,
        total_remaining: metrics.total_remaining,
        matched_per_second: metrics.matched_per_second,
    })
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

pub fn get_routes() -> Vec<Route> {
    openapi_get_routes![
        get_buy_orders,
        get_sell_orders,
        get_aggregated_buy_orders,
        get_aggregated_sell_orders,
        get_indexer_spread,
        get_aggregated_spread,
        get_orders_count,
        get_metrics
    ]
}

pub fn get_docs() -> SwaggerUIConfig {
    SwaggerUIConfig {
        url: "/openapi.json".to_string(),
        ..Default::default()
    }
}
