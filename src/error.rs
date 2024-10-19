use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to retrieve environment variable '{0}': {1}")]
    EnvVarError(String, String),

    #[error("Failed to match orders: {0}")]
    MatchOrdersError(String),

    #[error("Failed to send orders to matcher")]
    SendingToMatcherError,

    #[error("Failed to parse order amount: {0}")]
    OrderAmountParseError(String),

    #[error("Failed to parse contract ID")]
    ContractIdParseError(#[from] std::num::ParseIntError),

    #[error("String parsing error: {0}")]
    StringParsingError(String),

    #[error("Anyhow error: {0}")]
    AnyhowError(#[from] anyhow::Error),

    #[error("Url parse error {0}")]
    UrlParseError(#[from] url::ParseError),

    #[error("Chrono parse error {0}")]
    ChronoParseError(#[from] chrono::ParseError),

    #[error("Serde json error {0}")]
    SerdeJsonError(#[from] serde_json::Error),

    #[error("Tokio tungstenite error {0}")]
    TokioTungsteniteError(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Tokio tungstenite stream error {0}")]
    TokioTungsteniteStreamError(#[from] std::io::Error),

    #[error("Pangea client error {0}")]
    PangeaClientError(#[from] pangea_client::Error),
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Error::StringParsingError(s.to_string())
    }
}
