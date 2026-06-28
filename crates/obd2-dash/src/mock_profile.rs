/// Map CLI mock vehicle name to a VIN for spec matching.
pub fn mock_vin(name: &str) -> &'static str {
    match name.to_lowercase().as_str() {
        "mini" => "WMWRE33546T000001",
        "chevy" | "duramax" => "1GCHK23224F000001",
        "honda" | "accord" => "1HGCG32501A000001",
        _ => "00000000000000000",
    }
}
