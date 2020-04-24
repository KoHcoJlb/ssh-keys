use std::io::Cursor;

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use log::error;
use wrapperrs::{Error, ErrorExt, Result, ResultExt};

use wire::{ReadExt, WriteExt};

use crate::config::Config;
use crate::key::{KeyPair, PublicKey};
use crate::platform::ask_confirmation;

const SSH_AGENTC_REQUEST_IDENTITIES: u8 = 11;
const SSH_AGENTC_SIGN_REQUEST: u8 = 13;
const SSH_AGENTC_ADD_IDENTITY: u8 = 17;
#[allow(dead_code)]
const SSH_AGENTC_REMOVE_IDENTITY: u8 = 18;
#[allow(dead_code)]
const SSH_AGENTC_REMOVE_ALL_IDENTITIES: u8 = 19;
#[allow(dead_code)]
const SSH_AGENTC_ADD_ID_CONSTRAINED: u8 = 25;
#[allow(dead_code)]
const SSH_AGENTC_ADD_SMARTCARD_KEY: u8 = 20;
#[allow(dead_code)]
const SSH_AGENTC_REMOVE_SMARTCARD_KEY: u8 = 21;
#[allow(dead_code)]
const SSH_AGENTC_LOCK: u8 = 22;
#[allow(dead_code)]
const SSH_AGENTC_UNLOCK: u8 = 23;
#[allow(dead_code)]
const SSH_AGENTC_ADD_SMARTCARD_KEY_CONSTRAINED: u8 = 26;
#[allow(dead_code)]
const SSH_AGENTC_EXTENSION: u8 = 27;

const SSH_AGENT_FAILURE: u8 = 5;
const SSH_AGENT_SUCCESS: u8 = 6;
#[allow(dead_code)]
const SSH_AGENT_EXTENSION_FAILURE: u8 = 28;
const SSH_AGENT_IDENTITIES_ANSWER: u8 = 12;
const SSH_AGENT_SIGN_RESPONSE: u8 = 14;

pub mod wire;

pub struct Agent {
    config: Config,
}

#[derive(Debug)]
pub struct RequesterInfo {
    pub description_short: String,
    pub description_long: String,
}

#[derive(Debug)]
pub struct RequestInfo {
    pub channel: &'static str,
    pub requester: Option<RequesterInfo>,
}

impl Agent {
    pub fn new(config: Config) -> Agent {
        Agent { config }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    fn get_key(&self, public: &PublicKey) -> Option<(&KeyPair, usize)> {
        self.config
            .keys
            .iter()
            .enumerate()
            .find(|(_, key_pair)| key_pair.public() == public)
            .map(|(idx, key_pair)| (key_pair, idx))
    }

    pub fn add_key(&mut self, key_pair: KeyPair) -> Result<()> {
        if let None = self.get_key(key_pair.public()) {
            self.config.keys.push(key_pair);
            self.config.save().wrap_err("save config")?;
        };
        Ok(())
    }

    fn handle_request_internal(&mut self, buf: &[u8], info: &RequestInfo) -> Result<Vec<u8>> {
        let mut req = Cursor::new(buf);
        let mut resp = Vec::new();
        let msg_type = req.read_u8().wrap_err("read msg_type")?;
        (|| -> Result<()> {
            match msg_type {
                SSH_AGENTC_REQUEST_IDENTITIES => {
                    resp.write_u8(SSH_AGENT_IDENTITIES_ANSWER)?;
                    resp.write_u32::<BigEndian>(self.config.keys.len() as u32)?;
                    for key_pair in &self.config.keys {
                        resp.write_string(key_pair.public().encode())?;
                        resp.write_string(&key_pair.name())?;
                    }
                }
                SSH_AGENTC_ADD_IDENTITY => {
                    let key_pair = KeyPair::from_wire(&mut req).wrap_err("read key_pair")?;
                    self.add_key(key_pair).wrap_err("add key")?;
                    resp.write_u8(SSH_AGENT_SUCCESS)?;
                }
                SSH_AGENTC_SIGN_REQUEST => {
                    let pub_key = PublicKey::decode(&req.read_string()?)
                        .wrap_err("read public key")?;
                    let msg = req.read_string().wrap_err("read msg")?;
                    let flags = req.read_u32::<BigEndian>().wrap_err("read flags")?;
                    let (key_pair, _) = self.get_key(&pub_key)
                        .ok_or(Error::new("key not found"))?;

                    if ask_confirmation(key_pair, info, self.config()) {
                        resp.write_u8(SSH_AGENT_SIGN_RESPONSE)?;
                        resp.write_string(key_pair.private().sign(&msg, flags).wrap_err("sign")?)?;
                    } else {
                        resp.write_u8(SSH_AGENT_FAILURE)?;
                    }
                }
                _ => {
                    resp.write_u8(SSH_AGENT_FAILURE)?;
                }
            };
            Ok(())
        })().wrap_err(&format!("msg_type={}", msg_type))?;
        Ok(resp)
    }

    pub fn handle_request(&mut self, buf: &[u8], info: &RequestInfo) -> Vec<u8> {
        match self.handle_request_internal(buf, info) {
            Ok(resp) => resp,
            Err(err) => {
                error!("{}", err.wrap("error handling request"));
                vec![SSH_AGENT_FAILURE]
            }
        }
    }
}
