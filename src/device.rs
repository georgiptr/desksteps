//! Device address parsing and display, independent of Windows.
pub fn parse_address(value: &str) -> Result<u64, String> {
    let text = value.replace([':', '-'], "");
    if text.len() != 12 || !text.is_ascii() {
        return Err("Bluetooth address must contain 12 hex digits".into());
    }
    u64::from_str_radix(&text, 16).map_err(|_| "Invalid Bluetooth address".into())
}
pub fn address(value: u64) -> String {
    let text = format!("{value:012X}");
    (0..12)
        .step_by(2)
        .map(|i| &text[i..i + 2])
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn addresses_round_trip_and_format_consistently() {
        for value in [0, 1, 0x112233445566, 0xffffffffffff] {
            assert_eq!(parse_address(&address(value)).unwrap(), value);
        }
        assert_eq!(parse_address("11-22-33-44-55-ff").unwrap(), 0x1122334455ff);
    }
    #[test]
    fn malformed_and_non_ascii_addresses_are_rejected() {
        for value in [
            "",
            "112233",
            "11223344556677",
            "1122334455gg",
            "１２２３３４４５５６６７",
        ] {
            assert!(parse_address(value).is_err());
        }
    }
}
