use crate::indexer::spot_order::{OrderStatus, OrderType, SpotOrder};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct MatcherConnectRequest {
    pub uuid: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MatcherBatchRequest {
    pub uuid: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum MatcherRequest {
    BatchRequest(MatcherBatchRequest),
    OrderUpdates(Vec<MatcherOrderUpdate>),
    Connect(MatcherConnectRequest)
}

#[derive(Debug, Serialize, Deserialize)]
pub enum MatcherResponse {
    Batch(Vec<SpotOrder>),
    Ack,
    NoOrders
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatcherOrderUpdate {
    pub order_id: String,
    pub price: u128,
    pub timestamp: u64,
    pub new_amount: u128,
    pub status: Option<OrderStatus>,
    pub order_type: OrderType,
}
