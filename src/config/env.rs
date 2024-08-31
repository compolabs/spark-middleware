use std::env;

use crate::error::Error;

pub fn ev(key: &str) -> Result<String, Error> {
    env::var(key).map_err(|e| Error::EnvVarError(key.to_owned(), e.to_string()))
}
