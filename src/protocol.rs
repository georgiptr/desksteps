//! Verified Z1 handshake and decoder. See docs/PROTOCOL.md and docs/ATTRIBUTION.md.
use serde::{Deserialize, Serialize};

pub const MODEL_NAME: &str = "KS-HD-Z1D";
pub const TREADMILL: &str = "00002acd-0000-1000-8000-00805f9b34fb";
pub const SUPPLEMENT: &str = "24e2521c-f63b-48ed-85be-c5330b00fdf7";
pub const SUPPLEMENT_WRITE: &str = "24e2521c-f63b-48ed-85be-c5330d00fdf7";

pub fn handshake(name: &str, nonce: u8) -> Result<Vec<u8>, String> {
    if name != MODEL_NAME || nonce == 255 {
        return Err("Handshake requires the verified KS-HD-Z1D and nonce 0..254".into());
    }
    let mut bytes = vec![0x71, 0, 5, nonce];
    bytes.extend_from_slice(&(0x4431_5a2du32.wrapping_add(nonce.into())).to_le_bytes());
    bytes.push(bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)));
    Ok(bytes)
}

pub fn handshake_status(bytes: &[u8]) -> Option<u8> {
    (bytes.len() >= 3 && bytes[..2] == [0x71, 0x80]).then(|| bytes[2])
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Sample {
    pub flags: u16,
    pub speed_kmh: Option<f64>,
    pub average_speed_kmh: Option<f64>,
    pub distance_m: Option<u32>,
    pub inclination_percent: Option<f64>,
    pub ramp_angle_deg: Option<f64>,
    pub calories_kcal: Option<u32>,
    pub calories_per_hour: Option<u32>,
    pub heart_rate_bpm: Option<u32>,
    pub elapsed_s: Option<u32>,
    pub remaining_s: Option<u32>,
    pub steps: Option<u32>,
    pub unparsed_hex: String,
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], String> {
        let start = self.offset;
        if start + count > self.bytes.len() {
            return Err(format!(
                "Truncated FTMS packet at byte {start}, need {count}"
            ));
        }
        self.offset += count;
        Ok(&self.bytes[start..self.offset])
    }
    fn uint(&mut self, count: usize) -> Result<u32, String> {
        Ok(self
            .take(count)?
            .iter()
            .enumerate()
            .fold(0, |sum, (i, byte)| sum | ((*byte as u32) << (8 * i))))
    }
}

pub fn decode(bytes: &[u8]) -> Result<Sample, String> {
    let mut r = Reader { bytes, offset: 0 };
    let flags = r.uint(2)? as u16;
    let mut sample = Sample {
        flags,
        ..Default::default()
    };
    if flags & 1 == 0 {
        sample.speed_kmh = Some(r.uint(2)? as f64 / 100.0);
    }
    if flags & 2 != 0 {
        sample.average_speed_kmh = Some(r.uint(2)? as f64 / 100.0);
    }
    if flags & 4 != 0 {
        sample.distance_m = Some(r.uint(3)?);
    }
    if flags & 8 != 0 {
        let bytes = r.take(4)?;
        sample.inclination_percent = Some(i16::from_le_bytes([bytes[0], bytes[1]]) as f64 / 10.0);
        sample.ramp_angle_deg = Some(i16::from_le_bytes([bytes[2], bytes[3]]) as f64 / 10.0);
    }
    for (flag, count) in [(16, 4), (32, 1), (64, 1)] {
        if flags & flag != 0 {
            r.take(count)?;
        }
    }
    if flags & 128 != 0 {
        sample.calories_kcal = Some(r.uint(2)?);
        sample.calories_per_hour = Some(r.uint(2)?);
        r.take(1)?;
    }
    if flags & 256 != 0 {
        sample.heart_rate_bpm = Some(r.uint(1)?);
    }
    if flags & 512 != 0 {
        r.take(1)?;
    }
    if flags & 1024 != 0 {
        sample.elapsed_s = Some(r.uint(2)?);
    }
    if flags & 2048 != 0 {
        sample.remaining_s = Some(r.uint(2)?);
    }
    if flags & 4096 != 0 {
        r.take(4)?;
    }
    if flags & 8192 != 0 {
        sample.steps = Some(r.uint(3)?);
    }
    sample.unparsed_hex = hex(&bytes[r.offset..]);
    Ok(sample)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn unhex(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) || !value.is_ascii() {
        return Err("Invalid hex packet".into());
    }
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(|_| "Invalid hex packet".into()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_optional_field_has_independent_wire_values_and_truncation_checks() {
        let bytes =
            unhex("fe3f64006e00010203f6ffecff0102030405060700080009600a0b000c000d0e0f10010203aabb")
                .unwrap();
        let sample = decode(&bytes).unwrap();
        assert_eq!(sample.speed_kmh, Some(1.0));
        assert_eq!(sample.average_speed_kmh, Some(1.1));
        assert_eq!(sample.distance_m, Some(0x030201));
        assert_eq!(sample.inclination_percent, Some(-1.0));
        assert_eq!(sample.ramp_angle_deg, Some(-2.0));
        assert_eq!(sample.steps, Some(0x030201));
        assert_eq!(sample.unparsed_hex, "aabb");
        for size in 0..bytes.len() - 2 {
            assert!(decode(&bytes[..size]).is_err(), "truncation {size}");
        }
        for invalid in ["0", "zz", "éé"] {
            assert!(unhex(invalid).is_err());
        }
        assert_eq!(unhex("AAff").unwrap(), [0xaa, 0xff]);
        assert_eq!(handshake_status(&[]), None);
        assert_eq!(handshake_status(&[0x71, 0x81, 0]), None);
        assert_eq!(handshake_status(&[0x71, 0x80, 2]), Some(2));
    }
    #[test]
    fn handshake_matches_native_reference() {
        for (nonce, expected) in [
            (0, "710005002d5a314472"),
            (1, "710005012e5a314474"),
            (254, "710005fe2b5b31446f"),
        ] {
            assert_eq!(hex(&handshake(MODEL_NAME, nonce).unwrap()), expected);
        }
        assert!(handshake("other", 0).is_err());
        assert!(handshake(MODEL_NAME, 255).is_err());
        assert_eq!(handshake_status(&[0x71, 0x80, 0]), Some(0));
        assert_eq!(handshake_status(&[0x71, 0x80]), None);
    }
    #[test]
    fn upstream_capture_and_every_truncation() {
        let bytes = unhex("8424f401d2050068000000003a05060900").unwrap();
        let sample = decode(&bytes).unwrap();
        assert_eq!(sample.speed_kmh, Some(5.0));
        assert_eq!(sample.distance_m, Some(1490));
        assert_eq!(sample.calories_kcal, Some(104));
        assert_eq!(sample.elapsed_s, Some(1338));
        assert_eq!(sample.steps, Some(2310));
        for count in 0..bytes.len() {
            assert!(decode(&bytes[..count]).is_err());
        }
    }
    #[test]
    fn optional_fields_and_24_bit_counter() {
        let sample = decode(&unhex("05202a0000010000").unwrap()).unwrap();
        assert_eq!(sample.speed_kmh, None);
        assert_eq!(sample.distance_m, Some(42));
        assert_eq!(sample.steps, Some(1));
        assert_eq!(decode(&unhex("00006400").unwrap()).unwrap().steps, None);
        assert_eq!(
            decode(&unhex("00206400010001").unwrap()).unwrap().steps,
            Some(65537)
        );
        assert_eq!(
            decode(&unhex("00206400ffffff").unwrap()).unwrap().steps,
            Some(16777215)
        );
    }
}
