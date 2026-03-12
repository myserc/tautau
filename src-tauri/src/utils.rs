pub mod serde_hex {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(data: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(data))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        let mut arr = [0u8; 32];
        if let Ok(decoded) = hex::decode(s) {
            if decoded.len() == 32 { arr.copy_from_slice(&decoded); }
        }
        Ok(arr)
    }
}
pub mod serde_hex_vec {
    use serde::{Deserialize, Deserializer, Serializer, ser::SerializeSeq};
    pub fn serialize<S: Serializer>(data: &Vec<[u8; 32]>, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(data.len()))?;
        for e in data { seq.serialize_element(&hex::encode(e))?; }
        seq.end()
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<[u8; 32]>, D::Error> {
        let vec_s = Vec::<String>::deserialize(d)?;
        let mut res = Vec::new();
        for s in vec_s {
            let mut arr = [0u8; 32];
            if let Ok(decoded) = hex::decode(s) {
                if decoded.len() == 32 { arr.copy_from_slice(&decoded); res.push(arr); }
            }
        }
        Ok(res)
    }
}
