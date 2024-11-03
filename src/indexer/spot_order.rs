use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{self, Error};

// NTD Adapt spark-sdk OrderType to that type
#[derive(Debug, PartialEq, Eq, Clone, Copy, JsonSchema, Serialize, Deserialize)]
pub enum OrderType {
    Buy,
    Sell,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, JsonSchema, Serialize, Deserialize)]
pub enum LimitType {
    FOK,
    IOC,
    GTC,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, JsonSchema, Serialize, Deserialize)]
pub enum OrderStatus {
    New,
    PartiallyMatched,
    Matched,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, Eq)]
pub struct SpotOrder {
    pub id: String,
    pub user: String,
    pub asset: String,
    pub amount: u128,
    pub price: u128,
    pub timestamp: u64,
    pub order_type: OrderType,
    pub status: Option<OrderStatus>,
}

impl PartialEq for SpotOrder {
    fn eq(&self, other: &Self) -> bool {
        self.price == other.price
    }
}

impl Ord for SpotOrder {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.price
            .cmp(&other.price)
            .then_with(|| self.timestamp.cmp(&other.timestamp)) 
    }
}

impl PartialOrd for SpotOrder {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpotOrderEnvio {
    pub id: String,
    pub user: String,
    pub asset: String,
    pub amount: String,
    pub price: String,
    pub timestamp: String,
    pub order_type: OrderType,
    pub status: Option<String>,
    pub asset_type: Option<String>,
    pub db_write_timestamp: Option<String>,
    pub initial_amount: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubsquidOrder {
    pub id: String,
    pub asset: String,
    pub amount: String,
    pub price: String,
    pub timestamp: String,
    pub order_type: String,
    pub user: String,
    pub status: String,
    pub initial_amount: String,
}

impl SpotOrder {
    pub fn from_indexer_envio(intermediate: SpotOrderEnvio) -> Result<Self, Error> {
        let amount = intermediate.amount.parse::<u128>()?;
        let price = intermediate.price.parse::<u128>()?;
        let timestamp =
            chrono::DateTime::parse_from_rfc3339(&intermediate.timestamp)?.timestamp() as u64;

        Ok(SpotOrder {
            id: intermediate.id,
            user: intermediate.user,
            asset: intermediate.asset,
            amount,
            price,
            timestamp,
            order_type: intermediate.order_type,
            status: Some(OrderStatus::New),
        })
    }

    pub fn from_indexer_subsquid(order: SubsquidOrder) -> Result<Self, Error> {
        let amount = order.amount.parse::<u128>()?;
        let price = order.price.parse::<u128>()?;
        let timestamp = chrono::DateTime::parse_from_rfc3339(&order.timestamp)?.timestamp() as u64;

        let order_type = match order.order_type.as_str() {
            "Buy" => OrderType::Buy,
            "Sell" => OrderType::Sell,
            a => return Err(error::Error::UnknownOrderType(a.to_string())),
        };

        Ok(SpotOrder {
            id: order.id,
            user: order.user,
            asset: order.asset,
            amount,
            price,
            timestamp,
            order_type,
            status: Some(OrderStatus::New),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderPayloadSubsquid {
    pub active_buy_orders: Option<Vec<SubsquidOrder>>,
    pub active_sell_orders: Option<Vec<SubsquidOrder>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderPayloadEnvio {
    #[serde(rename = "ActiveBuyOrder")]
    pub active_buy_order: Option<Vec<SpotOrderEnvio>>,

    #[serde(rename = "ActiveSellOrder")]
    pub active_sell_order: Option<Vec<SpotOrderEnvio>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DataPayloadEnvio {
    pub data: OrderPayloadEnvio,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebSocketResponseEnvio {
    pub r#type: String,
    pub id: Option<String>,
    pub payload: Option<DataPayloadEnvio>,
}
