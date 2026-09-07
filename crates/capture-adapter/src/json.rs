//! Duplicate-key rejection at every nesting level, before typed decoding.
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Number, Value};
use std::fmt;
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("finite JSON without duplicate keys")
            }
            fn visit_bool<E: de::Error>(self, x: bool) -> Result<Unique, E> {
                Ok(Unique(Value::Bool(x)))
            }
            fn visit_i64<E: de::Error>(self, x: i64) -> Result<Unique, E> {
                Ok(Unique(Value::Number(x.into())))
            }
            fn visit_u64<E: de::Error>(self, x: u64) -> Result<Unique, E> {
                Ok(Unique(Value::Number(x.into())))
            }
            fn visit_f64<E: de::Error>(self, x: f64) -> Result<Unique, E> {
                Number::from_f64(x)
                    .map(|n| Unique(Value::Number(n)))
                    .ok_or_else(|| E::custom("nonfinite"))
            }
            fn visit_str<E: de::Error>(self, x: &str) -> Result<Unique, E> {
                Ok(Unique(Value::String(x.into())))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut v = Vec::new();
                while let Some(Unique(x)) = a.next_element()? {
                    v.push(x);
                }
                Ok(Unique(Value::Array(v)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut m = Map::new();
                while let Some((k, Unique(v))) = a.next_entry::<String, Unique>()? {
                    if m.insert(k, v).is_some() {
                        return Err(de::Error::custom("duplicate key"));
                    }
                }
                Ok(Unique(Value::Object(m)))
            }
        }
        d.deserialize_any(V)
    }
}
pub(crate) fn decode(bytes: &[u8]) -> crate::Result<Value> {
    serde_json::from_slice::<Unique>(bytes)
        .map(|v| v.0)
        .map_err(|_| crate::Error::new(crate::ErrorKind::InvalidJson))
}
