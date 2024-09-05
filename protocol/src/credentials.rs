use std::{borrow::Cow, fmt::Display, str::FromStr};

use aes_siv::aead::OsRng;
use aes_siv::{Aes256SivAead, KeyInit};
use anyhow::{format_err, Error};
use base64::display::Base64Display;
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use derivative::Derivative;
use generic_array::{typenum::U64, GenericArray};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Derivative)]
#[derivative(Debug)]
pub struct EncryptionKey(#[derivative(Debug = "ignore")] GenericArray<u8, U64>);

impl EncryptionKey {
    pub fn generate() -> Self {
        Self(Aes256SivAead::generate_key(&mut OsRng))
    }

    pub fn get(&self) -> &GenericArray<u8, U64> {
        &self.0
    }

    /// We intentionally don't implement `Display` on `Self` to prevent accidental
    /// encryption key disclosure (in logs, etc).
    pub fn fmt(&self) -> impl Display + '_ {
        Base64Display::new(self.0.as_ref(), &BASE64_URL_SAFE_NO_PAD)
    }
}

impl<'de> Deserialize<'de> for EncryptionKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Cow::<'_, str>::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

impl Serialize for EncryptionKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        BASE64_URL_SAFE_NO_PAD.encode(self.0).serialize(serializer)
    }
}

impl FromStr for EncryptionKey {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const KEY_LENGTH: usize = 64;

        let bytes = BASE64_URL_SAFE_NO_PAD.decode(s)?;
        let array = <[u8; KEY_LENGTH]>::try_from(bytes).map_err(|bytes| {
            format_err!("invalid length; got {}, expected {KEY_LENGTH}", bytes.len())
        })?;
        Ok(Self(array.into()))
    }
}
