use bumpalo::collections::vec::Vec;
use std::fmt::Debug;

/// In the WebAssembly binary format, all integers are variable-length encoded (using LEB-128)
/// A small value like 3 or 100 is encoded as 1 byte. The value 128 needs 2 bytes, etc.
/// In practice, this saves space, since small numbers used more often than large numbers.
/// Of course there is a price for this - an encoded U32 can be up to 5 bytes wide.
pub const MAX_SIZE_ENCODED_U32: usize = 5;
pub const MAX_SIZE_ENCODED_U64: usize = 10;

pub trait Serialize {
    fn serialize<T: SerialBuffer>(&self, buffer: &mut T);
}

impl Serialize for str {
    fn serialize<T: SerialBuffer>(&self, buffer: &mut T) {
        buffer.encode_u32(self.len() as u32);
        buffer.append_slice(self.as_bytes());
    }
}

impl Serialize for &str {
    fn serialize<T: SerialBuffer>(&self, buffer: &mut T) {
        buffer.encode_u32(self.len() as u32);
        buffer.append_slice(self.as_bytes());
    }
}

impl Serialize for u8 {
    fn serialize<T: SerialBuffer>(&self, buffer: &mut T) {
        buffer.append_u8(*self);
    }
}

impl Serialize for u32 {
    fn serialize<T: SerialBuffer>(&self, buffer: &mut T) {
        buffer.encode_u32(*self);
    }
}

// Unit is used as a placeholder in parts of the Wasm spec we don't use yet
impl Serialize for () {
    #[inline(always)]
    fn serialize<T: SerialBuffer>(&self, _buffer: &mut T) {}
}

impl<S: Serialize> Serialize for [S] {
    fn serialize<T: SerialBuffer>(&self, buffer: &mut T) {
        buffer.encode_u32(self.len() as u32);
        for item in self.iter() {
            item.serialize(buffer);
        }
    }
}

impl Serialize for Vec<'_, u8> {
    fn serialize<T: SerialBuffer>(&self, buffer: &mut T) {
        buffer.encode_u32(self.len() as u32);
        buffer.append_slice(self);
    }
}

impl<S: Serialize> Serialize for Option<S> {
    /// serialize Option as a vector of length 1 or 0
    fn serialize<T: SerialBuffer>(&self, buffer: &mut T) {
        match self {
            Some(x) => {
                buffer.append_u8(1);
                x.serialize(buffer);
            }
            None => {
                buffer.append_u8(0);
            }
        }
    }
}

impl<A: Serialize, B: Serialize> Serialize for (A, B) {
    fn serialize<T: SerialBuffer>(&self, buffer: &mut T) {
        self.0.serialize(buffer);
        self.1.serialize(buffer);
    }
}

/// Write an unsigned integer into the provided buffer in LEB-128 format, returning byte length
///
/// All integers in Wasm are variable-length encoded, which saves space for small values.
/// The most significant bit indicates "more bytes are coming", and the other 7 are payload.
macro_rules! encode_uleb128 {
    ($name: ident, $ty: ty) => {
        fn $name(&mut self, value: $ty) -> usize {
            let mut x = value;
            let start_len = self.size();
            while x >= 0x80 {
                self.append_u8(0x80 | ((x & 0x7f) as u8));
                x >>= 7;
            }
            self.append_u8(x as u8);
            self.size() - start_len
        }
    };
}

/// Write a signed integer into the provided buffer in LEB-128 format, returning byte length
macro_rules! encode_sleb128 {
    ($name: ident, $ty: ty) => {
        fn $name(&mut self, value: $ty) -> usize {
            let mut x = value;
            let start_len = self.size();
            loop {
                let byte = (x & 0x7f) as u8;
                x >>= 7;
                let byte_is_negative = (byte & 0x40) != 0;
                if ((x == 0 && !byte_is_negative) || (x == -1 && byte_is_negative)) {
                    self.append_u8(byte);
                    break;
                }
                self.append_u8(byte | 0x80);
            }
            self.size() - start_len
        }
    };
}

macro_rules! write_unencoded {
    ($name: ident, $ty: ty) => {
        /// write an unencoded little-endian integer (only used in relocations)
        fn $name(&mut self, value: $ty) {
            let mut x = value;
            let size = std::mem::size_of::<$ty>();
            for _ in 0..size {
                self.append_u8((x & 0xff) as u8);
                x >>= 8;
            }
        }
    };
}

/// For relocations
pub fn overwrite_padded_i32(buffer: &mut [u8], value: i32) {
    let mut x = value;
    for byte in buffer.iter_mut().take(4) {
        *byte = 0x80 | ((x & 0x7f) as u8);
        x >>= 7;
    }
    buffer[4] = (x & 0x7f) as u8;
}

pub fn overwrite_padded_u32(buffer: &mut [u8], value: u32) {
    let mut x = value;
    for byte in buffer.iter_mut().take(4) {
        *byte = 0x80 | ((x & 0x7f) as u8);
        x >>= 7;
    }
    buffer[4] = x as u8;
}

pub trait SerialBuffer: Debug {
    fn append_u8(&mut self, b: u8);
    fn overwrite_u8(&mut self, index: usize, b: u8);
    fn append_slice(&mut self, b: &[u8]);

    fn size(&self) -> usize;

    encode_uleb128!(encode_u32, u32);
    encode_uleb128!(encode_u64, u64);
    encode_sleb128!(encode_i32, i32);
    encode_sleb128!(encode_i64, i64);

    fn reserve_padded_u32(&mut self) -> usize;
    fn encode_padded_u32(&mut self, value: u32) -> usize;
    fn overwrite_padded_u32(&mut self, index: usize, value: u32);

    fn encode_f32(&mut self, value: f32) {
        self.write_unencoded_u32(value.to_bits());
    }

    fn encode_f64(&mut self, value: f64) {
        self.write_unencoded_u64(value.to_bits());
    }

    // methods for relocations
    write_unencoded!(write_unencoded_u32, u32);
    write_unencoded!(write_unencoded_u64, u64);
}

impl SerialBuffer for std::vec::Vec<u8> {
    fn append_u8(&mut self, b: u8) {
        self.push(b);
    }
    fn overwrite_u8(&mut self, index: usize, b: u8) {
        self[index] = b;
    }
    fn append_slice(&mut self, b: &[u8]) {
        self.extend_from_slice(b);
    }
    fn size(&self) -> usize {
        self.len()
    }
    fn reserve_padded_u32(&mut self) -> usize {
        let index = self.len();
        self.resize(index + MAX_SIZE_ENCODED_U32, 0xff);
        index
    }
    fn encode_padded_u32(&mut self, value: u32) -> usize {
        let index = self.len();
        let new_len = index + MAX_SIZE_ENCODED_U32;
        self.resize(new_len, 0);
        overwrite_padded_u32(&mut self[index..new_len], value);
        index
    }
    fn overwrite_padded_u32(&mut self, index: usize, value: u32) {
        overwrite_padded_u32(&mut self[index..(index + MAX_SIZE_ENCODED_U32)], value);
    }
}

impl<'a> SerialBuffer for Vec<'a, u8> {
    fn append_u8(&mut self, b: u8) {
        self.push(b);
    }
    fn overwrite_u8(&mut self, index: usize, b: u8) {
        self[index] = b;
    }
    fn append_slice(&mut self, b: &[u8]) {
        self.extend_from_slice(b);
    }
    fn size(&self) -> usize {
        self.len()
    }
    fn reserve_padded_u32(&mut self) -> usize {
        let index = self.len();
        self.resize(index + MAX_SIZE_ENCODED_U32, 0xff);
        index
    }
    fn encode_padded_u32(&mut self, value: u32) -> usize {
        let index = self.len();
        let new_len = index + MAX_SIZE_ENCODED_U32;
        self.resize(new_len, 0);
        overwrite_padded_u32(&mut self[index..new_len], value);
        index
    }
    fn overwrite_padded_u32(&mut self, index: usize, value: u32) {
        overwrite_padded_u32(&mut self[index..(index + MAX_SIZE_ENCODED_U32)], value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::{self, collections::Vec, Bump};

    fn help_u32(arena: &Bump, value: u32) -> Vec<'_, u8> {
        let mut buffer = Vec::with_capacity_in(MAX_SIZE_ENCODED_U32, arena);
        buffer.encode_u32(value);
        buffer
    }

    #[test]
    fn test_encode_u32() {
        let a = &Bump::new();

        // Edge cases
        assert_eq!(help_u32(a, 0), &[0]);
        assert_eq!(help_u32(a, u32::MAX), &[0xff, 0xff, 0xff, 0xff, 0x0f]);

        // Single-byte values
        assert_eq!(help_u32(a, 1), &[1]);
        assert_eq!(help_u32(a, 64), &[64]);
        assert_eq!(help_u32(a, 127), &[127]);

        // Two-byte values
        assert_eq!(help_u32(a, 128), &[0x80, 0x01]);
        assert_eq!(help_u32(a, 255), &[0xff, 0x01]);
        assert_eq!(help_u32(a, 256), &[0x80, 0x02]);
        assert_eq!(help_u32(a, 16383), &[0xff, 0x7f]);
        assert_eq!(help_u32(a, 16384), &[0x80, 0x80, 0x01]);

        // Three-byte values
        assert_eq!(help_u32(a, 16385), &[0x81, 0x80, 0x01]);
        assert_eq!(help_u32(a, 2097151), &[0xff, 0xff, 0x7f]);
        assert_eq!(help_u32(a, 2097152), &[0x80, 0x80, 0x80, 0x01]);

        // Four-byte values
        assert_eq!(help_u32(a, 2097153), &[0x81, 0x80, 0x80, 0x01]);
        assert_eq!(help_u32(a, 268435455), &[0xff, 0xff, 0xff, 0x7f]);
        assert_eq!(help_u32(a, 268435456), &[0x80, 0x80, 0x80, 0x80, 0x01]);

        // Five-byte values (only for very large numbers)
        assert_eq!(help_u32(a, 268435457), &[0x81, 0x80, 0x80, 0x80, 0x01]);
        assert_eq!(help_u32(a, 1073741823), &[0xff, 0xff, 0xff, 0xff, 0x03]);
        assert_eq!(help_u32(a, 1073741824), &[0x80, 0x80, 0x80, 0x80, 0x04]);

        // Some arbitrary values
        assert_eq!(help_u32(a, 1234567), &[0x87, 0xad, 0x4b]);
        assert_eq!(help_u32(a, 98765432), &[0xF8, 0x94, 0x8C, 0x2F]);
    }

    fn help_u64(arena: &Bump, value: u64) -> Vec<'_, u8> {
        let mut buffer = Vec::with_capacity_in(10, arena);
        buffer.encode_u64(value);
        buffer
    }

    #[test]
    fn test_encode_u64() {
        let a = &Bump::new();

        // Edge cases
        assert_eq!(help_u64(a, 0), &[0]);
        assert_eq!(
            help_u64(a, u64::MAX),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
        );

        // Single-byte values
        assert_eq!(help_u64(a, 1), &[1]);
        assert_eq!(help_u64(a, 64), &[64]);
        assert_eq!(help_u64(a, 127), &[127]);

        // Two-byte values
        assert_eq!(help_u64(a, 128), &[0x80, 0x01]);
        assert_eq!(help_u64(a, 255), &[0xff, 0x01]);
        assert_eq!(help_u64(a, 256), &[0x80, 0x02]);
        assert_eq!(help_u64(a, 16383), &[0xff, 0x7f]);
        assert_eq!(help_u64(a, 16384), &[0x80, 0x80, 0x01]);

        // Three-byte values
        assert_eq!(help_u64(a, 16385), &[0x81, 0x80, 0x01]);
        assert_eq!(help_u64(a, 2097151), &[0xff, 0xff, 0x7f]);
        assert_eq!(help_u64(a, 2097152), &[0x80, 0x80, 0x80, 0x01]);

        // Four-byte values
        assert_eq!(help_u64(a, 2097153), &[0x81, 0x80, 0x80, 0x01]);
        assert_eq!(help_u64(a, 268435455), &[0xff, 0xff, 0xff, 0x7f]);
        assert_eq!(help_u64(a, 268435456), &[0x80, 0x80, 0x80, 0x80, 0x01]);

        // Five-byte values
        assert_eq!(help_u64(a, 268435457), &[0x81, 0x80, 0x80, 0x80, 0x01]);
        assert_eq!(help_u64(a, 34359738367), &[0xff, 0xff, 0xff, 0xff, 0x7f]);
        assert_eq!(
            help_u64(a, 34359738368),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
        );

        // Six-byte values
        assert_eq!(
            help_u64(a, 34359738369),
            &[0x81, 0x80, 0x80, 0x80, 0x80, 0x01]
        );
        assert_eq!(
            help_u64(a, 4398046511103),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]
        );
        assert_eq!(
            help_u64(a, 4398046511104),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
        );

        // Seven-byte values
        assert_eq!(
            help_u64(a, 4398046511105),
            &[0x81, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
        );
        assert_eq!(
            help_u64(a, 562949953421311),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]
        );
        assert_eq!(
            help_u64(a, 562949953421312),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
        );

        // Eight-byte values
        assert_eq!(
            help_u64(a, 562949953421313),
            &[0x81, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
        );
        assert_eq!(
            help_u64(a, 72057594037927935),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]
        );
        assert_eq!(
            help_u64(a, 72057594037927936),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
        );

        // Nine-byte values
        assert_eq!(
            help_u64(a, 72057594037927937),
            &[0x81, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
        );
        assert_eq!(
            help_u64(a, 9223372036854775807),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]
        );
        assert_eq!(
            help_u64(a, 9223372036854775808),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
        );

        // Ten-byte values (only for very large numbers)
        assert_eq!(
            help_u64(a, 9223372036854775809),
            &[0x81, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
        );

        // Some arbitrary values
        assert_eq!(
            help_u64(a, 12345678901234),
            &[0xf2, 0xdf, 0xb8, 0x9e, 0xa7, 0xe7, 0x02]
        );
        assert_eq!(
            help_u64(a, 98765432109876543),
            &[0xbf, 0x8a, 0xfd, 0x87, 0xb2, 0xd4, 0xb8, 0xaf, 0x01]
        );

        // Values testing higher bits
        assert_eq!(
            help_u64(a, 1 << 63),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
        );
        assert_eq!(
            help_u64(a, (1 << 63) + 1),
            &[0x81, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
        );
    }

    fn help_i32(arena: &Bump, value: i32) -> Vec<'_, u8> {
        let mut buffer = Vec::with_capacity_in(MAX_SIZE_ENCODED_U32, arena);
        buffer.encode_i32(value);
        buffer
    }

    #[test]
    fn test_encode_i32() {
        let a = &Bump::new();

        // Edge cases
        assert_eq!(help_i32(a, 0), &[0]);
        assert_eq!(help_i32(a, i32::MAX), &[0xff, 0xff, 0xff, 0xff, 0x07]);
        assert_eq!(help_i32(a, i32::MIN), &[0x80, 0x80, 0x80, 0x80, 0x78]);

        // Single-byte positive values
        assert_eq!(help_i32(a, 1), &[1]);
        assert_eq!(help_i32(a, 63), &[63]);

        // Single-byte negative values
        assert_eq!(help_i32(a, -1), &[0x7f]);
        assert_eq!(help_i32(a, -64), &[0x40]);

        // Two-byte positive values
        assert_eq!(help_i32(a, 64), &[0xc0, 0x00]);
        assert_eq!(help_i32(a, 127), &[0xff, 0x00]);
        assert_eq!(help_i32(a, 128), &[0x80, 0x01]);
        assert_eq!(help_i32(a, 8191), &[0xff, 0x3f]);
        assert_eq!(help_i32(a, 8192), &[0x80, 0xC0, 0x00]);

        // Two-byte negative values
        assert_eq!(help_i32(a, -65), &[0xbf, 0x7f]);
        assert_eq!(help_i32(a, -128), &[0x80, 0x7f]);
        assert_eq!(help_i32(a, -129), &[0xff, 0x7e]);
        assert_eq!(help_i32(a, -8192), &[0x80, 0x40]);
        assert_eq!(help_i32(a, -8193), &[0xff, 0xbf, 0x7f]);

        // Three-byte positive values
        assert_eq!(help_i32(a, 16384), &[0x80, 0x80, 0x01]);
        assert_eq!(help_i32(a, 1048575), &[0xff, 0xff, 0x3f]);
        assert_eq!(help_i32(a, 1048576), &[0x80, 0x80, 0xC0, 0x00]);

        // Three-byte negative values
        assert_eq!(help_i32(a, -16385), &[0xff, 0xff, 0x7e]);
        assert_eq!(help_i32(a, -1048576), &[0x80, 0x80, 0x40]);
        assert_eq!(help_i32(a, -1048577), &[0xff, 0xff, 0xbf, 0x7f]);

        // Four-byte positive values
        assert_eq!(help_i32(a, 2097152), &[0x80, 0x80, 0x80, 0x01]);
        assert_eq!(help_i32(a, 134217727), &[0xff, 0xff, 0xff, 0x3f]);
        assert_eq!(help_i32(a, 134217728), &[0x80, 0x80, 0x80, 0xC0, 0x00]);

        // Four-byte negative values
        assert_eq!(help_i32(a, -2097153), &[0xff, 0xff, 0xff, 0x7e]);
        assert_eq!(help_i32(a, -134217728), &[0x80, 0x80, 0x80, 0x40]);
        assert_eq!(help_i32(a, -134217729), &[0xff, 0xff, 0xff, 0xbf, 0x7f]);

        // Five-byte values (only for very large positive or very small negative numbers)
        assert_eq!(help_i32(a, 268435456), &[0x80, 0x80, 0x80, 0x80, 0x01]);
        assert_eq!(help_i32(a, -268435457), &[0xff, 0xff, 0xff, 0xff, 0x7e]);

        // Some arbitrary values
        assert_eq!(help_i32(a, 123456), &[0xc0, 0xc4, 0x07]);
        assert_eq!(help_i32(a, -123456), &[0xc0, 0xbb, 0x78]);
        assert_eq!(help_i32(a, 9876543), &[0xbf, 0xe8, 0xda, 0x04]);
        assert_eq!(help_i32(a, -9876543), &[0xC1, 0x97, 0xa5, 0x7b]);

        // Values testing sign bit
        assert_eq!(help_i32(a, 0x3fffffff), &[0xff, 0xff, 0xff, 0xff, 0x03]);
        assert_eq!(help_i32(a, 0x40000000), &[0x80, 0x80, 0x80, 0x80, 0x04]);
        assert_eq!(help_i32(a, -0x40000000), &[0x80, 0x80, 0x80, 0x80, 0x7c]);
        assert_eq!(help_i32(a, -0x3fffffff), &[0x81, 0x80, 0x80, 0x80, 0x7c]);
    }

    fn help_i64(arena: &Bump, value: i64) -> Vec<'_, u8> {
        let mut buffer = Vec::with_capacity_in(10, arena);
        buffer.encode_i64(value);
        buffer
    }

    #[test]
    fn test_encode_i64() {
        let a = &Bump::new();

        // Edge cases
        assert_eq!(help_i64(a, 0), &[0]);
        assert_eq!(
            help_i64(a, i64::MAX),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00],
        );
        assert_eq!(
            help_i64(a, i64::MIN),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x7f],
        );

        // Single-byte positive values
        assert_eq!(help_i64(a, 1), &[1]);
        assert_eq!(help_i64(a, 63), &[63]);

        // Single-byte negative values
        assert_eq!(help_i64(a, -1), &[0x7f]);
        assert_eq!(help_i64(a, -64), &[0x40]);

        // Two-byte positive values
        assert_eq!(help_i64(a, 64), &[0xc0, 0x00]);
        assert_eq!(help_i64(a, 127), &[0xff, 0x00]);
        assert_eq!(help_i64(a, 128), &[0x80, 0x01]);
        assert_eq!(help_i64(a, 8191), &[0xff, 0x3f]);
        assert_eq!(help_i64(a, 8192), &[0x80, 0xC0, 0x00]);

        // Two-byte negative values
        assert_eq!(help_i64(a, -65), &[0xbf, 0x7f]);
        assert_eq!(help_i64(a, -128), &[0x80, 0x7f]);
        assert_eq!(help_i64(a, -129), &[0xff, 0x7e]);
        assert_eq!(help_i64(a, -8192), &[0x80, 0x40]);
        assert_eq!(help_i64(a, -8193), &[0xff, 0xbf, 0x7f]);

        // Three-byte values
        assert_eq!(help_i64(a, 16384), &[0x80, 0x80, 0x01]);
        assert_eq!(help_i64(a, -16385), &[0xff, 0xff, 0x7e]);
        assert_eq!(help_i64(a, 1048575), &[0xff, 0xff, 0x3f]);
        assert_eq!(help_i64(a, -1048576), &[0x80, 0x80, 0x40]);

        // Four-byte values
        assert_eq!(help_i64(a, 1048576), &[0x80, 0x80, 0xC0, 0x00]);
        assert_eq!(help_i64(a, -1048577), &[0xff, 0xff, 0xbf, 0x7f]);
        assert_eq!(help_i64(a, 134217727), &[0xff, 0xff, 0xff, 0x3f]);
        assert_eq!(help_i64(a, -134217728), &[0x80, 0x80, 0x80, 0x40]);

        // Five-byte values
        assert_eq!(help_i64(a, 134217728), &[0x80, 0x80, 0x80, 0xC0, 0x00]);
        assert_eq!(help_i64(a, -134217729), &[0xff, 0xff, 0xff, 0xbf, 0x7f]);
        assert_eq!(help_i64(a, 17179869183), &[0xff, 0xff, 0xff, 0xff, 0x3f]);
        assert_eq!(help_i64(a, -17179869184), &[0x80, 0x80, 0x80, 0x80, 0x40]);

        // Six-byte values
        assert_eq!(
            help_i64(a, 17179869184),
            &[0x80, 0x80, 0x80, 0x80, 0xc0, 0x00]
        );
        assert_eq!(
            help_i64(a, -17179869185),
            &[0xff, 0xff, 0xff, 0xff, 0xbf, 0x7f]
        );
        assert_eq!(
            help_i64(a, 2199023255551),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0x3f]
        );
        assert_eq!(
            help_i64(a, -2199023255552),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x40]
        );

        // Seven-byte values
        assert_eq!(
            help_i64(a, 2199023255552),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0xc0, 0x00]
        );
        assert_eq!(
            help_i64(a, -2199023255553),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xbf, 0x7f]
        );
        assert_eq!(
            help_i64(a, 281474976710655),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x3f]
        );
        assert_eq!(
            help_i64(a, -281474976710656),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x40]
        );

        // Eight-byte values
        assert_eq!(
            help_i64(a, 281474976710656),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0xc0, 0x00]
        );
        assert_eq!(
            help_i64(a, -281474976710657),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xbf, 0x7f]
        );
        assert_eq!(
            help_i64(a, 36028797018963967),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x3f]
        );
        assert_eq!(
            help_i64(a, -36028797018963968),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x40]
        );

        // Nine-byte values
        assert_eq!(
            help_i64(a, 36028797018963968),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0xc0, 0x00]
        );
        assert_eq!(
            help_i64(a, -36028797018963969),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xbf, 0x7f]
        );
        assert_eq!(
            help_i64(a, 4611686018427387903),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x3f]
        );
        assert_eq!(
            help_i64(a, -4611686018427387904),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x40]
        );

        // Ten-byte values (only for very large positive or very small negative numbers)
        assert_eq!(
            help_i64(a, 4611686018427387904),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0xc0, 0x00]
        );
        assert_eq!(
            help_i64(a, -4611686018427387905),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xbf, 0x7f]
        );

        // Some arbitrary values
        assert_eq!(help_i64(a, 123456789), &[0x95, 0x9a, 0xef, 0x3a]);
        assert_eq!(help_i64(a, -123456789), &[0xeb, 0xe5, 0x90, 0x45]);
        assert_eq!(help_i64(a, 9876543210), &[0xea, 0xad, 0xc0, 0xe5, 0x24]);
        assert_eq!(help_i64(a, -9876543210), &[0x96, 0xd2, 0xbf, 0x9a, 0x5b]);

        // Values testing sign bit and higher bits
        assert_eq!(
            help_i64(a, 0x3fffffffffffffff),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x3f]
        );
        assert_eq!(
            help_i64(a, 0x4000000000000000),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0xc0, 0x00]
        );
        assert_eq!(
            help_i64(a, -0x4000000000000000),
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x40]
        );
        assert_eq!(
            help_i64(a, -0x3fffffffffffffff),
            &[0x81, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x40]
        );
    }

    #[test]
    fn test_overwrite_u32_padded() {
        let mut buffer = [0, 0, 0, 0, 0];

        overwrite_padded_u32(&mut buffer, u32::MAX);
        assert_eq!(buffer, [0xff, 0xff, 0xff, 0xff, 0x0f]);

        overwrite_padded_u32(&mut buffer, 0);
        assert_eq!(buffer, [0x80, 0x80, 0x80, 0x80, 0x00]);

        overwrite_padded_u32(&mut buffer, 127);
        assert_eq!(buffer, [0xff, 0x80, 0x80, 0x80, 0x00]);

        overwrite_padded_u32(&mut buffer, 128);
        assert_eq!(buffer, [0x80, 0x81, 0x80, 0x80, 0x00]);
    }

    #[test]
    fn test_write_unencoded_u32() {
        let mut buffer = std::vec::Vec::with_capacity(4);

        // Edge cases
        buffer.write_unencoded_u32(0);
        assert_eq!(buffer, &[0, 0, 0, 0]);
        buffer.clear();

        buffer.write_unencoded_u32(u32::MAX);
        assert_eq!(buffer, &[0xff, 0xff, 0xff, 0xff]);
        buffer.clear();

        // Testing each byte individually
        buffer.write_unencoded_u32(0x000000FF);
        assert_eq!(buffer, &[0xff, 0, 0, 0]);
        buffer.clear();

        buffer.write_unencoded_u32(0x0000FF00);
        assert_eq!(buffer, &[0, 0xff, 0, 0]);
        buffer.clear();

        buffer.write_unencoded_u32(0x00FF0000);
        assert_eq!(buffer, &[0, 0, 0xff, 0]);
        buffer.clear();

        buffer.write_unencoded_u32(0xFF000000);
        assert_eq!(buffer, &[0, 0, 0, 0xff]);
        buffer.clear();

        // Testing combinations of bytes
        buffer.write_unencoded_u32(0x0000FFFF);
        assert_eq!(buffer, &[0xff, 0xff, 0, 0]);
        buffer.clear();

        buffer.write_unencoded_u32(0xFFFF0000);
        assert_eq!(buffer, &[0, 0, 0xff, 0xff]);
        buffer.clear();

        buffer.write_unencoded_u32(0xFF00FF00);
        assert_eq!(buffer, &[0, 0xff, 0, 0xff]);
        buffer.clear();

        // Some arbitrary values
        buffer.write_unencoded_u32(0x12345678);
        assert_eq!(buffer, &[0x78, 0x56, 0x34, 0x12]);
        buffer.clear();

        buffer.write_unencoded_u32(0xABCDEF01);
        assert_eq!(buffer, &[0x01, 0xef, 0xcd, 0xab]);
        buffer.clear();

        // Testing endianness
        buffer.write_unencoded_u32(0x01020304);
        assert_eq!(buffer, &[0x04, 0x03, 0x02, 0x01]);
        buffer.clear();

        // Additional edge cases
        buffer.write_unencoded_u32(1);
        assert_eq!(buffer, &[1, 0, 0, 0]);
        buffer.clear();

        buffer.write_unencoded_u32(u32::MAX - 1);
        assert_eq!(buffer, &[0xfe, 0xff, 0xff, 0xff]);
        buffer.clear();

        // Powers of two
        buffer.write_unencoded_u32(0x00000001);
        assert_eq!(buffer, &[0x01, 0x00, 0x00, 0x00]);
        buffer.clear();

        buffer.write_unencoded_u32(0x00010000);
        assert_eq!(buffer, &[0x00, 0x00, 0x01, 0x00]);
        buffer.clear();

        // Large values
        buffer.write_unencoded_u32(0x7FFFFFFF); // Largest positive signed 32-bit integer
        assert_eq!(buffer, &[0xff, 0xff, 0xff, 0x7f]);
        buffer.clear();

        buffer.write_unencoded_u32(0x80000000); // Smallest negative signed 32-bit integer when interpreted as signed
        assert_eq!(buffer, &[0x00, 0x00, 0x00, 0x80]);
        buffer.clear();
    }

    #[test]
    fn test_write_unencoded_u64() {
        let mut buffer = std::vec::Vec::with_capacity(8);

        buffer.write_unencoded_u64(0);
        assert_eq!(buffer, &[0, 0, 0, 0, 0, 0, 0, 0]);

        buffer.clear();
        buffer.write_unencoded_u64(u64::MAX);
        assert_eq!(buffer, &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    }

    fn help_pad_i32(val: i32) -> [u8; MAX_SIZE_ENCODED_U32] {
        let mut buffer = [0; MAX_SIZE_ENCODED_U32];
        overwrite_padded_i32(&mut buffer, val);
        buffer
    }

    #[test]
    fn test_encode_padded_i32() {
        assert_eq!(help_pad_i32(0), [0x80, 0x80, 0x80, 0x80, 0x00]);
        assert_eq!(help_pad_i32(1), [0x81, 0x80, 0x80, 0x80, 0x00]);
        assert_eq!(help_pad_i32(-1), [0xff, 0xff, 0xff, 0xff, 0x7f]);
        assert_eq!(help_pad_i32(i32::MAX), [0xff, 0xff, 0xff, 0xff, 0x07]);
        assert_eq!(help_pad_i32(i32::MIN), [0x80, 0x80, 0x80, 0x80, 0x78]);

        let mut buffer = [0xff; 10];
        overwrite_padded_i32(&mut buffer[2..], 0);
        assert_eq!(
            buffer,
            [0xff, 0xff, 0x80, 0x80, 0x80, 0x80, 0x00, 0xff, 0xff, 0xff]
        );
    }
}
