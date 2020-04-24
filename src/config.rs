use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use wrapperrs::Result;

use crate::key::KeyPair;
use crate::platform::config_dir;

#[derive(Deserialize, Serialize)]
pub struct Config {
    #[serde(
    deserialize_with = "crate::key::deserialize_key_pairs",
    serialize_with = "crate::key::serialize_key_pairs"
    )]
    pub keys: Vec<KeyPair>,
}

impl Default for Config {
    fn default() -> Self {
        Config { keys: Vec::new() }
    }
}

impl Config {
    pub fn save(&self) -> Result<()> {
        let mut file = File::create(config_file())?;
        file.write_all(&toml::to_string_pretty(self)?.as_bytes())?;
        Ok(())
    }

    pub fn reload(&mut self) -> Result<()> {
        *self = load_config()?;
        Ok(())
    }
}

fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn load_config() -> Result<Config> {
    match File::open(config_file()) {
        Ok(mut file) => {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            Ok(toml::from_slice(&buf)?)
        }
        Err(err) => {
            if let ErrorKind::NotFound = err.kind() {
                Ok(Config::default())
            } else {
                Err(err.into())
            }
        }
    }
}
