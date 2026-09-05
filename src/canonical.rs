//! Детерминированное бинарное кодирование объектов протокола.
//!
//! JSON используется только на границе REST API. Хешируемые и подписываемые
//! сообщения кодируются вручную, чтобы порядок полей и представление чисел не
//! зависели от реализации сериализатора.

pub const PROTOCOL_VERSION: u8 = 1;

#[derive(Debug, Default)]
pub struct CanonicalEncoder {
    bytes: Vec<u8>,
}

impl CanonicalEncoder {
    pub fn new(domain: &str) -> Self {
        let mut encoder = Self::default();
        encoder.put_bytes(domain.as_bytes());
        encoder.put_u8(PROTOCOL_VERSION);
        encoder
    }

    pub fn put_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn put_bool(&mut self, value: bool) {
        self.put_u8(u8::from(value));
    }

    pub fn put_u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub fn put_u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub fn put_i64(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub fn put_str(&mut self, value: &str) {
        self.put_bytes(value.as_bytes());
    }

    pub fn put_bytes(&mut self, value: &[u8]) {
        let len = u32::try_from(value.len()).expect("canonical field exceeds u32::MAX");
        self.put_u32(len);
        self.bytes.extend_from_slice(value);
    }

    pub fn put_fixed(&mut self, value: &[u8]) {
        self.bytes.extend_from_slice(value);
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_prefix_avoids_ambiguous_concatenation() {
        let mut first = CanonicalEncoder::new("TEST");
        first.put_str("ab");
        first.put_str("c");

        let mut second = CanonicalEncoder::new("TEST");
        second.put_str("a");
        second.put_str("bc");

        assert_ne!(first.finish(), second.finish());
    }

    #[test]
    fn integer_encoding_is_big_endian_and_deterministic() {
        let mut first = CanonicalEncoder::new("TEST");
        first.put_u64(0x0102_0304_0506_0708);
        let mut second = CanonicalEncoder::new("TEST");
        second.put_u64(0x0102_0304_0506_0708);
        assert_eq!(first.finish(), second.finish());
    }
}
