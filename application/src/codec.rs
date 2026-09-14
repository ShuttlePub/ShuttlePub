use bytes::Bytes;
use nitinol::eventsource::codec::Codec;
use serde::{Serialize, de::DeserializeOwned};

#[derive(Default)]
pub struct JsonCodec;

impl<T: Serialize + DeserializeOwned> Codec<T> for JsonCodec {
    type Error = serde_json::Error;

    fn encode(value: &T) -> Result<Bytes, Self::Error> {
        serde_json::to_vec(value).map(Bytes::from)
    }

    fn decode(payload: &[u8]) -> Result<T, Self::Error> {
        serde_json::from_slice(payload)
    }
}
