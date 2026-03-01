//! Lightweight VIN decoder for when no database match exists.
//!
//! Decodes model year from position 10 and manufacturer from the WMI (positions 1-3).

/// Decoded VIN information.
#[derive(Debug, Clone)]
pub struct DecodedVin {
    pub year: Option<i32>,
    pub manufacturer: Option<String>,
}

/// Decode model year from VIN position 10 (1-indexed).
///
/// Covers 2010–2039 per the standard VIN year encoding:
/// A=2010 .. H=2017, J=2018 .. N=2022, P=2023, R=2024 .. T=2026,
/// V=2027 .. Y=2030, 1=2031 .. 9=2039
pub fn decode_year(vin: &str) -> Option<i32> {
    let ch = vin.as_bytes().get(9)?; // position 10, 0-indexed = 9
    match ch {
        b'A' => Some(2010),
        b'B' => Some(2011),
        b'C' => Some(2012),
        b'D' => Some(2013),
        b'E' => Some(2014),
        b'F' => Some(2015),
        b'G' => Some(2016),
        b'H' => Some(2017),
        b'J' => Some(2018),
        b'K' => Some(2019),
        b'L' => Some(2020),
        b'M' => Some(2021),
        b'N' => Some(2022),
        b'P' => Some(2023),
        b'R' => Some(2024),
        b'S' => Some(2025),
        b'T' => Some(2026),
        b'V' => Some(2027),
        b'W' => Some(2028),
        b'X' => Some(2029),
        b'Y' => Some(2030),
        b'1' => Some(2031),
        b'2' => Some(2032),
        b'3' => Some(2033),
        b'4' => Some(2034),
        b'5' => Some(2035),
        b'6' => Some(2036),
        b'7' => Some(2037),
        b'8' => Some(2038),
        b'9' => Some(2039),
        _ => None,
    }
}

/// Decode manufacturer from WMI (positions 1-3).
pub fn decode_manufacturer(vin: &str) -> Option<&'static str> {
    if vin.len() < 3 {
        return None;
    }
    let wmi = &vin[..3];
    // Match on full 3-char WMI first, then fall back to first character
    match wmi {
        // GM
        "1G1" | "1G2" | "1GC" | "1GK" | "2G1" | "3G1" => Some("Chevrolet"),
        "1GM" | "1GY" => Some("Cadillac"),
        "1GT" => Some("GMC"),
        "1G3" | "1G4" => Some("Buick"),
        // Ford
        "1FA" | "1FB" | "1FC" | "1FD" | "1FM" | "1FT" | "3FA" => Some("Ford"),
        "1LN" | "3LN" => Some("Lincoln"),
        // Chrysler / Stellantis
        "1C3" | "1C6" | "2C3" | "2C4" => Some("Chrysler/Dodge"),
        "1C4" | "1J4" | "1J8" => Some("Jeep"),
        // Toyota
        "1NX" | "2T1" | "4T1" | "5TD" | "JTD" | "JTE" | "JTN" => Some("Toyota"),
        "5J6" | "5J8" | "JHM" => Some("Honda"),
        "JN1" | "JN8" | "1N4" | "1N6" | "5N1" => Some("Nissan"),
        "4S3" | "4S4" | "JF1" | "JF2" => Some("Subaru"),
        "JM1" | "JM3" => Some("Mazda"),
        "KM8" | "KMH" | "5NP" | "5NM" => Some("Hyundai"),
        "KNA" | "KND" | "5XY" => Some("Kia"),
        // European
        "WBA" | "WBS" | "WBY" | "5UX" => Some("BMW"),
        "WDB" | "WDD" | "WDC" | "WMX" | "55S" => Some("Mercedes-Benz"),
        "WAU" | "WA1" => Some("Audi"),
        "WVW" | "WV2" | "3VW" => Some("Volkswagen"),
        "WP0" | "WP1" => Some("Porsche"),
        "ZFF" | "ZAM" => Some("Ferrari/Maserati"),
        "YV1" | "YV4" => Some("Volvo"),
        "SAL" | "SAJ" => Some("Jaguar/Land Rover"),
        // MINI
        "WMW" => Some("MINI"),
        _ => {
            // Fall back to country of origin from first character
            match vin.as_bytes()[0] {
                b'1' | b'4' | b'5' => Some("US Manufacturer"),
                b'2' => Some("Canadian Manufacturer"),
                b'3' => Some("Mexican Manufacturer"),
                b'J' => Some("Japanese Manufacturer"),
                b'K' => Some("Korean Manufacturer"),
                b'W' => Some("German Manufacturer"),
                b'S' => Some("British Manufacturer"),
                b'Z' => Some("Italian Manufacturer"),
                b'Y' => Some("Swedish/Finnish Manufacturer"),
                _ => None,
            }
        }
    }
}

/// Decode a VIN into year and manufacturer.
pub fn decode(vin: &str) -> DecodedVin {
    DecodedVin {
        year: decode_year(vin),
        manufacturer: decode_manufacturer(vin).map(String::from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_year() {
        assert_eq!(decode_year("1G1ZD5ST2LF000000"), Some(2020)); // L = 2020
                                                                  // WMWRE33546T — position 10 (1-indexed) = '6', which maps to 2036 in the
                                                                  // 2010+ table. The 30-year VIN cycle means '6' also meant 2006, but our
                                                                  // decoder only covers the current cycle.
        assert_eq!(decode_year("WMWRE33546T000001"), Some(2036));
        assert_eq!(decode_year("SHORT"), None);
        assert_eq!(decode_year("123456789A1234567"), Some(2010)); // A = 2010
    }

    #[test]
    fn test_decode_manufacturer() {
        assert_eq!(decode_manufacturer("1G1ZD5ST2LF000000"), Some("Chevrolet"));
        assert_eq!(decode_manufacturer("WMWRE33546T000001"), Some("MINI"));
        // Tesla (5YJ) is not in the specific WMI table, but '5' falls back to "US Manufacturer"
        assert_eq!(
            decode_manufacturer("5YJSA1E26MF000001"),
            Some("US Manufacturer")
        );
        assert_eq!(decode_manufacturer("WBA12345678901234"), Some("BMW"));
        assert_eq!(decode_manufacturer("AB"), None); // Too short
    }

    #[test]
    fn test_decode_full() {
        let decoded = decode("1G1ZD5ST2LF000000");
        assert_eq!(decoded.year, Some(2020));
        assert_eq!(decoded.manufacturer.as_deref(), Some("Chevrolet"));
    }
}
