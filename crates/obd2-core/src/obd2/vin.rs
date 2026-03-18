//! Lightweight VIN decoder for when no database match exists.
//!
//! Decodes model year from position 10 and manufacturer from the WMI (positions 1-3).
//! Handles both the current (2010–2039) and previous (1980–2009) 30-year VIN year cycles.

/// Decoded VIN information.
#[derive(Debug, Clone)]
pub struct DecodedVin {
    /// Model year from the current cycle (2010–2039).
    pub year: Option<i32>,
    /// Model year from the previous cycle (1980–2009), if the code is ambiguous.
    pub year_alt: Option<i32>,
    pub manufacturer: Option<String>,
}

/// Decode model year from VIN position 10 (1-indexed).
///
/// Returns the current-cycle (2010–2039) year. For pre-2010 vehicles, use
/// `decode_year_candidates()` which returns both possible years.
pub fn decode_year(vin: &str) -> Option<i32> {
    decode_year_candidates(vin).0
}

/// Decode model year returning both 30-year cycle candidates.
///
/// Returns `(current_cycle, previous_cycle)` where:
/// - current_cycle = 2010–2039
/// - previous_cycle = 1980–2009
///
/// Letters A–Y map to 1980–2000 / 2010–2030 (skipping I, O, Q, U, Z).
/// Digits 1–9 map to 2001–2009 / 2031–2039.
pub fn decode_year_candidates(vin: &str) -> (Option<i32>, Option<i32>) {
    let ch = match vin.as_bytes().get(9) {
        Some(c) => c,
        None => return (None, None),
    };
    match ch {
        b'A' => (Some(2010), Some(1980)),
        b'B' => (Some(2011), Some(1981)),
        b'C' => (Some(2012), Some(1982)),
        b'D' => (Some(2013), Some(1983)),
        b'E' => (Some(2014), Some(1984)),
        b'F' => (Some(2015), Some(1985)),
        b'G' => (Some(2016), Some(1986)),
        b'H' => (Some(2017), Some(1987)),
        b'J' => (Some(2018), Some(1988)),
        b'K' => (Some(2019), Some(1989)),
        b'L' => (Some(2020), Some(1990)),
        b'M' => (Some(2021), Some(1991)),
        b'N' => (Some(2022), Some(1992)),
        b'P' => (Some(2023), Some(1993)),
        b'R' => (Some(2024), Some(1994)),
        b'S' => (Some(2025), Some(1995)),
        b'T' => (Some(2026), Some(1996)),
        b'V' => (Some(2027), Some(1997)),
        b'W' => (Some(2028), Some(1998)),
        b'X' => (Some(2029), Some(1999)),
        b'Y' => (Some(2030), Some(2000)),
        b'1' => (Some(2031), Some(2001)),
        b'2' => (Some(2032), Some(2002)),
        b'3' => (Some(2033), Some(2003)),
        b'4' => (Some(2034), Some(2004)),
        b'5' => (Some(2035), Some(2005)),
        b'6' => (Some(2036), Some(2006)),
        b'7' => (Some(2037), Some(2007)),
        b'8' => (Some(2038), Some(2008)),
        b'9' => (Some(2039), Some(2009)),
        _ => (None, None),
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
        // Honda
        "1HG" | "2HG" | "5J6" | "5J8" | "JHG" | "JHM" | "SHH" => Some("Honda"),
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
    let (year, year_alt) = decode_year_candidates(vin);
    DecodedVin {
        year,
        year_alt,
        manufacturer: decode_manufacturer(vin).map(String::from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_year() {
        assert_eq!(decode_year("1G1ZD5ST2LF000000"), Some(2020)); // L = 2020
        assert_eq!(decode_year("WMWRE33546T000001"), Some(2036)); // 6 = 2036 (current cycle)
        assert_eq!(decode_year("SHORT"), None);
        assert_eq!(decode_year("123456789A1234567"), Some(2010)); // A = 2010
    }

    #[test]
    fn test_decode_year_candidates() {
        // '6' → 2036 (current) or 2006 (previous) — the MINI is actually 2006
        let (current, prev) = decode_year_candidates("WMWRE33546T000001");
        assert_eq!(current, Some(2036));
        assert_eq!(prev, Some(2006));

        // '1' → 2031 (current) or 2001 (previous) — the Accord is 2001
        let (current, prev) = decode_year_candidates("1HGCG32501A000001");
        assert_eq!(current, Some(2031));
        assert_eq!(prev, Some(2001));

        // 'L' → 2020 (current) or 1990 (previous)
        let (current, prev) = decode_year_candidates("1G1ZD5ST2LF000000");
        assert_eq!(current, Some(2020));
        assert_eq!(prev, Some(1990));

        // Short VIN
        assert_eq!(decode_year_candidates("SHORT"), (None, None));
    }

    #[test]
    fn test_decode_manufacturer() {
        assert_eq!(decode_manufacturer("1G1ZD5ST2LF000000"), Some("Chevrolet"));
        assert_eq!(decode_manufacturer("WMWRE33546T000001"), Some("MINI"));
        // Honda — US-built Accord (1HG WMI)
        assert_eq!(decode_manufacturer("1HGCG32501A000001"), Some("Honda"));
        // Honda — Alabama plant
        assert_eq!(decode_manufacturer("5J6YH2H73PL000001"), Some("Honda"));
        // Tesla (5YJ) falls back to "US Manufacturer"
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
        assert_eq!(decoded.year_alt, Some(1990));
        assert_eq!(decoded.manufacturer.as_deref(), Some("Chevrolet"));

        // Honda Accord 2001
        let decoded = decode("1HGCG32501A000001");
        assert_eq!(decoded.year, Some(2031));
        assert_eq!(decoded.year_alt, Some(2001));
        assert_eq!(decoded.manufacturer.as_deref(), Some("Honda"));
    }
}
