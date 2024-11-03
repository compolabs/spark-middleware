use std::num::ParseIntError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Fuel error: {0}")]
    Fuel(#[from] fuels::types::errors::Error),

    #[error("Failed to retrieve environment variable '{0}': {1}")]
    EnvVarError(String, String),

    #[error("Failed to match orders: {0}")]
    MatchOrdersError(String),

    #[error("Failed to send orders to matcher")]
    SendingToMatcherError,

    #[error("Unknown order type: {0}")]
    UnknownOrderType(String),

    #[error("Anyhow error: {0}")]
    AnyhowError(#[from] anyhow::Error),

    #[error("Serde json error {0}")]
    SerdeJsonError(#[from] serde_json::Error),

    #[error("Tokio tungstenite error {0}")]
    TokioTungsteniteError(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Tokio tungstenite stream error {0}")]
    TokioTungsteniteStreamError(#[from] std::io::Error),

    #[error("Pangea client error {0}")]
    PangeaClientError(#[from] pangea_client::Error),

    #[error("Parsing error: {0}")]
    ParsingError(#[from] ParsingError),

    #[error("Unknown chain id")]
    UnknownChainIdError,

    #[error("Pangea ws max retries exceeded")]
    MaxRetriesExceeded
}

#[derive(Error, Debug)]
pub enum ParsingError {
    #[error("Url parse error {0}")]
    UrlParseError(#[from] url::ParseError),

    #[error("String parsing error: {0}")]
    StringParsingError(String),

    #[error("Chrono parse error {0}")]
    ChronoParseError(#[from] chrono::ParseError),

    #[error("Failed to parse order amount: {0}")]
    OrderAmountParseError(String),

    #[error("Failed to parse contract ID")]
    ContractIdParseError(#[from] std::num::ParseIntError),

    #[error("From Hex Error")]
    FromHexError(#[from] rustc_hex::FromHexError),

    #[error(transparent)]
    FromUtf8Error(#[from] std::string::FromUtf8Error),
}

macro_rules! impl_from_error {
    ($($source:ty),*) => {
        $(
            impl From<$source> for Error {
                fn from(err: $source) -> Self {
                    Error::ParsingError(err.into())
                }
            }
        )*
    };
}

impl_from_error!(
    url::ParseError,
    chrono::ParseError,
    ParseIntError,
    rustc_hex::FromHexError,
    std::string::FromUtf8Error
);
