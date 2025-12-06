use {
    aes_siv::{aead::array::Array, Aes256SivAead, Key, KeyInit},
    anyhow::{anyhow, bail, ensure, format_err, Error},
    base64::{display::Base64Display, prelude::BASE64_URL_SAFE_NO_PAD, Engine},
    generic_array::typenum::U64,
    rand::{
        distr::{Alphanumeric, SampleString},
        rand_core,
        rngs::OsRng,
        CryptoRng,
    },
    serde::{de, Deserialize, Deserializer, Serialize, Serializer},
    std::{
        any::Any,
        borrow::Cow,
        fmt::{self, Debug, Display},
        panic::catch_unwind,
        str::FromStr,
    },
};

/// Secret token used by the client to identify itself and gain access to the server API.
///
/// Each client should have a separate access token.
#[derive(Clone, Deserialize, Serialize)]
pub struct AccessToken(String);

const ACCESS_TOKEN_LENGTH: usize = 64;

fn format_panic_message(err: &(dyn Any + Send + 'static)) -> String {
    err.downcast_ref::<&'static str>()
        .map(|&s| s.to_owned())
        .or_else(|| err.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| format!("{err:?}"))
}

impl AccessToken {
    #[inline]
    pub fn generate() -> anyhow::Result<Self> {
        catch_unwind(|| {
            Self(Alphanumeric.sample_string(&mut rand_core::UnwrapErr(OsRng), ACCESS_TOKEN_LENGTH))
        })
        .map_err(|err| anyhow!(format_panic_message(&*err)))
    }

    #[must_use]
    #[inline]
    pub fn as_unmasked_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for AccessToken {
    type Err = Error;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ensure!(
            s.len() == ACCESS_TOKEN_LENGTH,
            "invalid length; got {}, expected {ACCESS_TOKEN_LENGTH}",
            s.len(),
        );
        if let Some(c) = s.chars().find(|c| !c.is_ascii_alphanumeric()) {
            bail!("must be alphanumeric but contains invalid character `{c}`");
        }
        Ok(Self(s.to_owned()))
    }
}

impl Debug for AccessToken {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessToken").finish()
    }
}

/// Secret used to encrypt file contents and metadata.
///
/// All clients of a user should use the same encryption key.
#[derive(Clone)]
pub struct EncryptionKey(Array<u8, U64>);

impl EncryptionKey {
    #[inline]
    pub fn generate() -> anyhow::Result<Self> {
        Ok(Self(Aes256SivAead::generate_key()?))
    }

    #[inline]
    pub fn generate_with_rng<R: CryptoRng + ?Sized>(rng: &mut R) -> Self {
        let mut key = Key::<Aes256SivAead>::default();
        rng.fill_bytes(&mut key);
        Self(key)
    }

    #[must_use]
    #[inline]
    pub fn get(&self) -> &Array<u8, U64> {
        &self.0
    }

    #[must_use]
    #[inline]
    pub fn display_unmasked(&self) -> impl Display + '_ {
        Base64Display::new(self.0.as_ref(), &BASE64_URL_SAFE_NO_PAD)
    }
}

impl<'de> Deserialize<'de> for EncryptionKey {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Cow::<'_, str>::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

impl Serialize for EncryptionKey {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        BASE64_URL_SAFE_NO_PAD.encode(self.0).serialize(serializer)
    }
}

impl FromStr for EncryptionKey {
    type Err = Error;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const KEY_LENGTH: usize = 64;

        let bytes = BASE64_URL_SAFE_NO_PAD.decode(s)?;
        let array = <[u8; KEY_LENGTH]>::try_from(bytes).map_err(|bytes| {
            format_err!("invalid length; got {}, expected {KEY_LENGTH}", bytes.len())
        })?;
        Ok(Self(array.into()))
    }
}

impl Debug for EncryptionKey {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncryptionKey").finish()
    }
}

#[cfg(test)]
#[expect(clippy::string_slice, reason = "test")]
mod test {
    use super::*;

    #[test]
    fn access_token_from_str() {
        static TOKEN: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ab";
        assert_eq!(
            AccessToken::from_str(TOKEN).unwrap().as_unmasked_str(),
            TOKEN,
        );
        AccessToken::from_str("").unwrap_err();
        AccessToken::from_str(&TOKEN[1..]).unwrap_err();
        AccessToken::from_str(&format!("{TOKEN}c")).unwrap_err();
        AccessToken::from_str(&format!("{}:", &TOKEN[1..])).unwrap_err();
    }

    #[test]
    fn encryption_key_from_str() {
        static KEY: &str = "qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqg";
        assert_eq!(
            EncryptionKey::from_str(KEY)
                .unwrap()
                .display_unmasked()
                .to_string(),
            KEY,
        );
        EncryptionKey::from_str("").unwrap_err();
        EncryptionKey::from_str("qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqo").unwrap_err();
        EncryptionKey::from_str(&format!("{KEY}:")).unwrap_err();
    }
}
