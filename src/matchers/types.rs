use crate::indexer::spot_order::SpotOrder;
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
    pub orders: Vec<String>,
}

#[derive(Deserialize)]
pub struct MatcherConnectRequest {
    pub uuid: String,
}
