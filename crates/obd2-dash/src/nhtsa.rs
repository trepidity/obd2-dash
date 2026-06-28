//! NHTSA vPIC VIN Decoder API client.
//!
//! Decodes any US-market VIN into full vehicle info using the free
//! NHTSA Vehicle Product Information Catalog API.
//! <https://vpic.nhtsa.dot.gov/api/>

use obd2_db::models::VehicleInfo;
use serde::Deserialize;

const API_BASE: &str = "https://vpic.nhtsa.dot.gov/api/vehicles/decodevin";

/// Timeout for the NHTSA API call. Vehicle init shouldn't block forever.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, Deserialize)]
struct NhtsaResponse {
    #[serde(rename = "Results")]
    results: Vec<NhtsaResult>,
}

#[derive(Debug, Deserialize)]
struct NhtsaResult {
    #[serde(rename = "Variable")]
    variable: String,
    #[serde(rename = "Value")]
    value: Option<String>,
}

/// Result of an NHTSA VIN decode, with the fields we care about.
#[derive(Debug, Clone)]
pub struct NhtsaVehicle {
    pub make: Option<String>,
    pub model: Option<String>,
    pub year: Option<i32>,
    pub trim: Option<String>,
    pub fuel_type: Option<String>,
    pub displacement_l: Option<f64>,
    pub cylinders: Option<i32>,
    pub drive_type: Option<String>,
    pub transmission_type: Option<String>,
    pub engine_model: Option<String>,
    pub body_class: Option<String>,
}

impl NhtsaVehicle {
    /// Classify this vehicle into a threshold category based on its attributes.
    /// Returns a category code used as a `scope_id` for engine_family threshold overrides.
    pub fn threshold_category(&self) -> &'static str {
        let is_diesel = self
            .fuel_type
            .as_deref()
            .map(|f| f.to_lowercase().contains("diesel"))
            .unwrap_or(false);

        let displacement = self.displacement_l.unwrap_or(0.0);
        let cylinders = self.cylinders.unwrap_or(0);

        match (is_diesel, cylinders, displacement) {
            // Diesel trucks
            (true, _, d) if d >= 5.0 => "diesel-truck",
            (true, _, _) => "diesel-truck", // any diesel with known cyl count

            // Gas V10
            (false, 10, _) => "gas-truck-v10",

            // Gas V8 (large displacement truck engines)
            (false, 8, d) if d >= 5.0 => "gas-truck-v8",

            // Gas V8 (smaller, e.g. 4.8L Vortec)
            (false, 8, _) => "gas-truck-v8",

            // Gas V6 trucks
            (false, 6, d) if d >= 3.0 => "gas-truck-v6",

            // Fallback — use generic defaults
            _ => "generic",
        }
    }

    /// Convert to a `VehicleInfo` for database storage.
    pub fn to_vehicle_info(&self, vin: &str) -> VehicleInfo {
        let category = self.threshold_category();
        VehicleInfo {
            vin: vin.to_string(),
            year: self.year,
            make: self.make.clone(),
            model: self.model.clone(),
            trim: self.trim.clone(),
            engine_family_id: None,
            engine_family_code: if category != "generic" {
                Some(category.to_string())
            } else {
                None
            },
            transmission_type: self.transmission_type.clone(),
            drive_type: self.drive_type.clone(),
            fuel_type: self.fuel_type.clone(),
            displacement_l: self.displacement_l,
            cylinders: self.cylinders,
        }
    }
}

/// Decode a VIN using the NHTSA vPIC API.
///
/// Returns `Ok(Some(vehicle))` on success, `Ok(None)` if the API returned no useful data,
/// and `Err` on network/parse failures.
pub async fn decode_vin(vin: &str) -> Result<Option<NhtsaVehicle>, String> {
    let url = format!("{}/{}?format=json", API_BASE, vin);
    tracing::debug!(target: "obd2::nhtsa", "Requesting VIN decode: {}", vin);

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| {
            tracing::error!(target: "obd2::nhtsa", "Failed to build HTTP client: {}", e);
            format!("HTTP client error: {e}")
        })?;

    let resp = client.get(&url).send().await.map_err(|e| {
        tracing::warn!(target: "obd2::nhtsa", "NHTSA request failed for {}: {}", vin, e);
        format!("NHTSA request failed: {e}")
    })?;

    if !resp.status().is_success() {
        tracing::warn!(target: "obd2::nhtsa", "NHTSA returned status {} for VIN {}", resp.status(), vin);
        return Err(format!("NHTSA returned status {}", resp.status()));
    }

    let body: NhtsaResponse = resp.json().await.map_err(|e| {
        tracing::warn!(target: "obd2::nhtsa", "Failed to parse NHTSA response for {}: {}", vin, e);
        format!("NHTSA parse error: {e}")
    })?;

    let vehicle = parse_nhtsa_results(&body.results);

    // Only return if we got at least make + model
    if vehicle.make.is_some() && vehicle.model.is_some() {
        tracing::info!(
            target: "obd2::nhtsa",
            "Decoded VIN {}: {} {} {} ({:?}, {:?}L, {}cyl)",
            vin,
            vehicle.year.unwrap_or(0),
            vehicle.make.as_deref().unwrap_or("?"),
            vehicle.model.as_deref().unwrap_or("?"),
            vehicle.fuel_type.as_deref().unwrap_or("?"),
            vehicle.displacement_l.unwrap_or(0.0),
            vehicle.cylinders.unwrap_or(0),
        );
        Ok(Some(vehicle))
    } else {
        tracing::info!(target: "obd2::nhtsa", "NHTSA returned no useful data for VIN {}", vin);
        Ok(None)
    }
}

fn parse_nhtsa_results(results: &[NhtsaResult]) -> NhtsaVehicle {
    let mut vehicle = NhtsaVehicle {
        make: None,
        model: None,
        year: None,
        trim: None,
        fuel_type: None,
        displacement_l: None,
        cylinders: None,
        drive_type: None,
        transmission_type: None,
        engine_model: None,
        body_class: None,
    };

    for r in results {
        let val = match &r.value {
            Some(v) if !v.is_empty() => v.clone(),
            _ => continue,
        };

        match r.variable.as_str() {
            "Make" => vehicle.make = Some(val),
            "Model" => vehicle.model = Some(val),
            "Model Year" => vehicle.year = val.parse().ok(),
            "Trim" => vehicle.trim = Some(val),
            "Fuel Type - Primary" => vehicle.fuel_type = normalize_fuel_type(&val),
            "Displacement (L)" => vehicle.displacement_l = val.parse().ok(),
            "Engine Number of Cylinders" => vehicle.cylinders = val.parse().ok(),
            "Drive Type" => vehicle.drive_type = Some(val),
            "Transmission Style" => vehicle.transmission_type = Some(val),
            "Engine Model" => vehicle.engine_model = Some(val),
            "Body Class" => vehicle.body_class = Some(val),
            _ => {}
        }
    }

    vehicle
}

/// Normalize NHTSA fuel type strings to our standard values.
fn normalize_fuel_type(raw: &str) -> Option<String> {
    let lower = raw.to_lowercase();
    if lower.contains("diesel") {
        Some("Diesel".to_string())
    } else if lower.contains("gasoline") || lower.contains("gas") {
        Some("Gasoline".to_string())
    } else if lower.contains("e85") || lower.contains("flex") {
        Some("E85/Flex".to_string())
    } else {
        Some(raw.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nhtsa_results() {
        let results = vec![
            NhtsaResult {
                variable: "Make".to_string(),
                value: Some("Ford".to_string()),
            },
            NhtsaResult {
                variable: "Model".to_string(),
                value: Some("F-250 SD".to_string()),
            },
            NhtsaResult {
                variable: "Model Year".to_string(),
                value: Some("2020".to_string()),
            },
            NhtsaResult {
                variable: "Fuel Type - Primary".to_string(),
                value: Some("Diesel".to_string()),
            },
            NhtsaResult {
                variable: "Displacement (L)".to_string(),
                value: Some("6.7".to_string()),
            },
            NhtsaResult {
                variable: "Engine Number of Cylinders".to_string(),
                value: Some("8".to_string()),
            },
            NhtsaResult {
                variable: "Drive Type".to_string(),
                value: Some("4WD".to_string()),
            },
            NhtsaResult {
                variable: "Empty Field".to_string(),
                value: None,
            },
        ];

        let v = parse_nhtsa_results(&results);
        assert_eq!(v.make.as_deref(), Some("Ford"));
        assert_eq!(v.model.as_deref(), Some("F-250 SD"));
        assert_eq!(v.year, Some(2020));
        assert_eq!(v.fuel_type.as_deref(), Some("Diesel"));
        assert_eq!(v.displacement_l, Some(6.7));
        assert_eq!(v.cylinders, Some(8));
    }

    #[test]
    fn test_threshold_category_diesel_truck() {
        let v = NhtsaVehicle {
            make: Some("Ford".into()),
            model: Some("F-350".into()),
            year: Some(2020),
            fuel_type: Some("Diesel".into()),
            displacement_l: Some(6.7),
            cylinders: Some(8),
            ..empty_vehicle()
        };
        assert_eq!(v.threshold_category(), "diesel-truck");
    }

    #[test]
    fn test_threshold_category_gas_v8() {
        let v = NhtsaVehicle {
            make: Some("Chevrolet".into()),
            model: Some("Silverado 2500HD".into()),
            year: Some(2022),
            fuel_type: Some("Gasoline".into()),
            displacement_l: Some(6.6),
            cylinders: Some(8),
            ..empty_vehicle()
        };
        assert_eq!(v.threshold_category(), "gas-truck-v8");
    }

    #[test]
    fn test_threshold_category_gas_v10() {
        let v = NhtsaVehicle {
            make: Some("Ford".into()),
            model: Some("F-350".into()),
            year: Some(2010),
            fuel_type: Some("Gasoline".into()),
            displacement_l: Some(6.8),
            cylinders: Some(10),
            ..empty_vehicle()
        };
        assert_eq!(v.threshold_category(), "gas-truck-v10");
    }

    #[test]
    fn test_to_vehicle_info() {
        let v = NhtsaVehicle {
            make: Some("RAM".into()),
            model: Some("3500".into()),
            year: Some(2019),
            fuel_type: Some("Diesel".into()),
            displacement_l: Some(6.7),
            cylinders: Some(6),
            ..empty_vehicle()
        };
        let info = v.to_vehicle_info("3C63R3HL0KG000001");
        assert_eq!(info.vin, "3C63R3HL0KG000001");
        assert_eq!(info.engine_family_code.as_deref(), Some("diesel-truck"));
    }

    #[test]
    fn test_normalize_fuel_type() {
        assert_eq!(normalize_fuel_type("Diesel").as_deref(), Some("Diesel"));
        assert_eq!(normalize_fuel_type("Gasoline").as_deref(), Some("Gasoline"));
        assert_eq!(
            normalize_fuel_type("Flexible Fuel Vehicle (FFV)").as_deref(),
            Some("E85/Flex")
        );
    }

    fn empty_vehicle() -> NhtsaVehicle {
        NhtsaVehicle {
            make: None,
            model: None,
            year: None,
            trim: None,
            fuel_type: None,
            displacement_l: None,
            cylinders: None,
            drive_type: None,
            transmission_type: None,
            engine_model: None,
            body_class: None,
        }
    }
}
