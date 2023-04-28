use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use rammingen::config::EncryptionKey;

fn main() {
    let key = EncryptionKey::generate();
    println!("{}", BASE64_URL_SAFE_NO_PAD.encode(key.0));
}
