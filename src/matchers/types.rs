use crate::indexer::spot_order::{OrderStatus, OrderType, SpotOrder};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub enum MatcherRequest {
    Orders(Vec<SpotOrder>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MatcherResponseWrapper {
    pub MatchResult: MatcherResponse,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MatcherResponse {
    pub success: bool,
    pub orders: Vec<MatcherOrderUpdate>, 
}

#[derive(Deserialize)]
pub struct MatcherConnectRequest {
    pub uuid: String,
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
