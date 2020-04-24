use std::io::{Cursor, Read};

use openssl::{bn::BigNum, hash::MessageDigest, pkey::PKey, pkey::Private, rsa::Rsa, sign::Signer};
use wrapperrs::{Error, Result, ResultExt};

pub use ser::*;

use crate::agent::wire::{ReadExt, WriteExt};

mod ser;

pub enum PrivateKey {
    RSA(Rsa<Private>),
}

pub enum PublicKey {
    RSA { e: BigNum, n: BigNum },
}

pub struct KeyPair {
    private: PrivateKey,
    public: PublicKey,
    name: String,
}

impl PrivateKey {
    fn from_wire<R: Read>(r: &mut R) -> Result<PrivateKey> {
        let key_type = r.read_string_utf8()?;
        match key_type.as_str() {
            "ssh-rsa" => {
                let n = r.read_mpint()?;
                let e = r.read_mpint()?;
                let d = r.read_mpint()?;
                let iqmp = r.read_mpint()?;
                let p = r.read_mpint()?;
                let q = r.read_mpint()?;

                let one = BigNum::from_u32(1).unwrap();
                let dp = &d % &(&p - &one);
                let dq = &d % &(&q - &one);

                Ok(PrivateKey::RSA(
                    Rsa::from_private_components(n, e, d, p, q, dp, dq, iqmp)
                        .wrap_err("create key")?,
                ))
            }
            _ => Err(Error::new(&format!("unknown key type: {}", key_type)).into()),
        }
    }

    fn public(&self) -> PublicKey {
        use PrivateKey::*;

        match self {
            RSA(key) => PublicKey::RSA {
                e: key.e().to_owned().unwrap(),
                n: key.n().to_owned().unwrap(),
            },
        }
    }

    pub fn sign(&self, msg: &[u8], flags: u32) -> Result<Vec<u8>> {
        use PrivateKey::*;

        match self {
            RSA(key) => {
                let pkey = PKey::from_rsa(key.clone()).wrap_err("create pkey")?;

                let (digest, sig_type) = match 1 {
                    _ if flags & 0x4 > 0 => (MessageDigest::sha512(), "rsa-sha2-512"),
                    _ if flags & 0x2 > 0 => (MessageDigest::sha256(), "rsa-sha2-256"),
                    _ => (MessageDigest::sha1(), "ssh-rsa"),
                };

                let mut signer = Signer::new(digest, &pkey).wrap_err("create signer")?;

                let mut sig = Vec::new();
                sig.write_string(sig_type)?;
                sig.write_string(signer.sign_oneshot_to_vec(msg)?)?;
                Ok(sig)
            }
        }
    }
}

impl PublicKey {
    pub fn key_type(&self) -> &str {
        use PublicKey::*;
        match self {
            RSA { .. } => "ssh-rsa",
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        use PublicKey::*;

        match self {
            RSA { e, n } => {
                let mut buf = Vec::new();
                buf.write_string("ssh-rsa").unwrap();
                buf.write_mpint(e).unwrap();
                buf.write_mpint(n).unwrap();
                buf
            }
        }
    }

    pub fn decode(buf: &[u8]) -> Result<PublicKey> {
        let mut cur = Cursor::new(buf);
        match cur.read_string_utf8()?.as_str() {
            "ssh-rsa" => Ok(PublicKey::RSA {
                e: cur.read_mpint()?,
                n: cur.read_mpint()?,
            }),
            key_type => Err(Error::new(&format!("unknown key type: {}", key_type)).into()),
        }
    }
}

impl PartialEq<PublicKey> for PublicKey {
    fn eq(&self, other: &PublicKey) -> bool {
        use PublicKey::*;

        match self {
            RSA { n, e } => {
                let RSA { e: e1, n: n1 } = other;
                n == n1 && e == e1
            }
        }
    }
}

impl KeyPair {
    pub fn new(private_key: PrivateKey, name: String) -> KeyPair {
        KeyPair {
            public: private_key.public(),
            private: private_key,
            name,
        }
    }

    pub fn from_wire<R: Read>(r: &mut R) -> Result<KeyPair> {
        let key = PrivateKey::from_wire(r).wrap_err("read key")?;
        let name = r.read_string_utf8()?;
        Ok(KeyPair {
            public: key.public(),
            private: key,
            name,
        })
    }

    pub fn private(&self) -> &PrivateKey {
        &self.private
    }

    pub fn public(&self) -> &PublicKey {
        &self.public
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}
