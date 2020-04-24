use std::fmt;

use openssl::rsa::Rsa;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde::de::MapAccess;
use serde::ser::SerializeMap;

use crate::key::{KeyPair, PrivateKey};

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "lowercase")]
enum KeyType {
    RSA,
}

#[derive(Deserialize, Serialize, Debug)]
struct KeyConfig {
    #[serde(rename = "type")]
    key_type: KeyType,
    data: String,
}

pub fn deserialize_key_pairs<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<KeyPair>, D::Error> {
    use serde::de::Error;

    struct Visitor;

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = Vec<KeyPair>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("private key")
        }

        fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
        {
            use KeyType::*;

            let mut v = Vec::new();
            while let Some((key, config)) = map.next_entry::<_, KeyConfig>()? {
                let private = match config.key_type {
                    RSA => PrivateKey::RSA(
                        Rsa::private_key_from_pem(config.data.as_bytes())
                            .or(Err(A::Error::custom("invalid data")))?,
                    ),
                };
                v.push(KeyPair::new(private, key));
            }
            Ok(v)
        }
    }

    deserializer.deserialize_map(Visitor)
}

pub fn serialize_key_pairs<S: Serializer>(
    v: &Vec<KeyPair>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(None)?;
    for key_pair in v {
        use PrivateKey::*;
        map.serialize_key(key_pair.name())?;

        let (key_type, data) = match &key_pair.private {
            RSA(rsa) => (
                KeyType::RSA,
                String::from_utf8(rsa.private_key_to_pem().unwrap()).unwrap(),
            ),
        };
        map.serialize_value(&KeyConfig { key_type, data })?;
    }
    map.end()
}
