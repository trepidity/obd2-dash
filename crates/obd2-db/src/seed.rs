use anyhow::Result;

use crate::models::{DefaultThreshold, EngineFamily, PidThreshold, VehicleInfo};
use crate::Database;

/// Seed all reference data into the database.
pub fn seed_all(db: &Database) -> Result<()> {
    seed_engine_families(db)?;
    seed_vehicles(db)?;
    seed_default_thresholds(db)?;
    seed_engine_family_overrides(db)?;
    Ok(())
}

fn seed_engine_families(db: &Database) -> Result<()> {
    // BMW/Tritec W11B16 — 1.6L Supercharged I4 (Mini Cooper S)
    db.upsert_engine_family(&EngineFamily {
        id: None,
        manufacturer: "BMW/Tritec".to_string(),
        family_code: "W11B16".to_string(),
        displacement_l: 1.6,
        cylinders: 4,
        layout: "I4".to_string(),
        aspiration: "Supercharged".to_string(),
        fuel_type: "Gasoline".to_string(),
        compression_ratio: Some(8.3),
        redline_rpm: Some(6800),
        idle_rpm_cold: Some(1100),
        idle_rpm_warm: Some(750),
        max_power_kw: Some(125.0),
        max_torque_nm: Some(220.0),
    })?;

    // GM/Isuzu LLY — 6.6L Turbo V8 (Duramax)
    db.upsert_engine_family(&EngineFamily {
        id: None,
        manufacturer: "GM/Isuzu".to_string(),
        family_code: "LLY".to_string(),
        displacement_l: 6.6,
        cylinders: 8,
        layout: "V8".to_string(),
        aspiration: "Turbocharged".to_string(),
        fuel_type: "Diesel".to_string(),
        compression_ratio: Some(17.5),
        redline_rpm: Some(3200),
        idle_rpm_cold: Some(900),
        idle_rpm_warm: Some(650),
        max_power_kw: Some(224.0),
        max_torque_nm: Some(890.0),
    })?;

    // GM LFV — 1.5L Turbo I4 (Chevrolet Malibu)
    db.upsert_engine_family(&EngineFamily {
        id: None,
        manufacturer: "GM".to_string(),
        family_code: "LFV".to_string(),
        displacement_l: 1.5,
        cylinders: 4,
        layout: "I4".to_string(),
        aspiration: "Turbocharged".to_string(),
        fuel_type: "Gasoline".to_string(),
        compression_ratio: Some(10.0),
        redline_rpm: Some(6500),
        idle_rpm_cold: Some(1100),
        idle_rpm_warm: Some(700),
        max_power_kw: Some(119.0),  // 160 hp
        max_torque_nm: Some(250.0), // 184 lb-ft
    })?;

    // GM LSY — 2.0L Turbo I4 (Chevrolet Malibu Premier)
    db.upsert_engine_family(&EngineFamily {
        id: None,
        manufacturer: "GM".to_string(),
        family_code: "LSY".to_string(),
        displacement_l: 2.0,
        cylinders: 4,
        layout: "I4".to_string(),
        aspiration: "Turbocharged".to_string(),
        fuel_type: "Gasoline".to_string(),
        compression_ratio: Some(9.5),
        redline_rpm: Some(6500),
        idle_rpm_cold: Some(1100),
        idle_rpm_warm: Some(700),
        max_power_kw: Some(186.0),  // 250 hp
        max_torque_nm: Some(353.0), // 260 lb-ft
    })?;

    // Honda F23A1 — 2.3L SOHC VTEC I4 (Accord)
    db.upsert_engine_family(&EngineFamily {
        id: None,
        manufacturer: "Honda".to_string(),
        family_code: "F23A1".to_string(),
        displacement_l: 2.3,
        cylinders: 4,
        layout: "I4".to_string(),
        aspiration: "Natural".to_string(),
        fuel_type: "Gasoline".to_string(),
        compression_ratio: Some(9.3),
        redline_rpm: Some(6100),
        idle_rpm_cold: Some(1200),
        idle_rpm_warm: Some(750),
        max_power_kw: Some(112.0),  // 150 hp
        max_torque_nm: Some(207.0), // 152 lb-ft
    })?;

    // Honda J30A1 — 3.0L SOHC VTEC V6 (Accord V6)
    db.upsert_engine_family(&EngineFamily {
        id: None,
        manufacturer: "Honda".to_string(),
        family_code: "J30A1".to_string(),
        displacement_l: 3.0,
        cylinders: 6,
        layout: "V6".to_string(),
        aspiration: "Natural".to_string(),
        fuel_type: "Gasoline".to_string(),
        compression_ratio: Some(9.4),
        redline_rpm: Some(6200),
        idle_rpm_cold: Some(1200),
        idle_rpm_warm: Some(700),
        max_power_kw: Some(149.0),  // 200 hp
        max_torque_nm: Some(268.0), // 197 lb-ft
    })?;

    Ok(())
}

fn seed_vehicles(db: &Database) -> Result<()> {
    // Look up engine family IDs
    let w11_id = db
        .get_engine_family_code_id("W11B16")?
        .expect("W11B16 should be seeded");
    let lly_id = db
        .get_engine_family_code_id("LLY")?
        .expect("LLY should be seeded");
    let lfv_id = db
        .get_engine_family_code_id("LFV")?
        .expect("LFV should be seeded");
    let lsy_id = db
        .get_engine_family_code_id("LSY")?
        .expect("LSY should be seeded");
    let f23a1_id = db
        .get_engine_family_code_id("F23A1")?
        .expect("F23A1 should be seeded");

    // 2006 MINI Cooper S
    db.upsert_vehicle(&VehicleInfo {
        vin: "WMWRE33546T000001".to_string(),
        year: Some(2006),
        make: Some("MINI".to_string()),
        model: Some("Cooper S".to_string()),
        trim: Some("S".to_string()),
        engine_family_id: Some(w11_id),
        engine_family_code: Some("W11B16".to_string()),
        transmission_type: Some("Manual".to_string()),
        drive_type: Some("FWD".to_string()),
        fuel_type: Some("Gasoline".to_string()),
        displacement_l: Some(1.6),
        cylinders: Some(4),
    })?;

    // 2004 Chevrolet Silverado 2500HD
    db.upsert_vehicle(&VehicleInfo {
        vin: "1GCHK23164F000001".to_string(),
        year: Some(2004),
        make: Some("Chevrolet".to_string()),
        model: Some("Silverado 2500HD".to_string()),
        trim: Some("LT".to_string()),
        engine_family_id: Some(lly_id),
        engine_family_code: Some("LLY".to_string()),
        transmission_type: Some("Automatic".to_string()),
        drive_type: Some("4WD".to_string()),
        fuel_type: Some("Diesel".to_string()),
        displacement_l: Some(6.6),
        cylinders: Some(8),
    })?;

    // 2020 Chevrolet Malibu LT (1.5L Turbo LFV, CVT)
    db.upsert_vehicle(&VehicleInfo {
        vin: "1G1ZD5ST0LF000001".to_string(),
        year: Some(2020),
        make: Some("Chevrolet".to_string()),
        model: Some("Malibu".to_string()),
        trim: Some("LT".to_string()),
        engine_family_id: Some(lfv_id),
        engine_family_code: Some("LFV".to_string()),
        transmission_type: Some("CVT".to_string()),
        drive_type: Some("FWD".to_string()),
        fuel_type: Some("Gasoline".to_string()),
        displacement_l: Some(1.5),
        cylinders: Some(4),
    })?;

    // 2020 Chevrolet Malibu Premier (2.0L Turbo LSY, 9-speed auto)
    db.upsert_vehicle(&VehicleInfo {
        vin: "1G1ZH5ST0LF000002".to_string(),
        year: Some(2020),
        make: Some("Chevrolet".to_string()),
        model: Some("Malibu".to_string()),
        trim: Some("Premier".to_string()),
        engine_family_id: Some(lsy_id),
        engine_family_code: Some("LSY".to_string()),
        transmission_type: Some("Automatic 9-speed".to_string()),
        drive_type: Some("FWD".to_string()),
        fuel_type: Some("Gasoline".to_string()),
        displacement_l: Some(2.0),
        cylinders: Some(4),
    })?;

    // 2001 Honda Accord Coupe (2.3L I4 F23A1)
    db.upsert_vehicle(&VehicleInfo {
        vin: "1HGCG32501A000001".to_string(),
        year: Some(2001),
        make: Some("Honda".to_string()),
        model: Some("Accord".to_string()),
        trim: Some("EX Coupe".to_string()),
        engine_family_id: Some(f23a1_id),
        engine_family_code: Some("F23A1".to_string()),
        transmission_type: Some("Automatic 4-speed".to_string()),
        drive_type: Some("FWD".to_string()),
        fuel_type: Some("Gasoline".to_string()),
        displacement_l: Some(2.3),
        cylinders: Some(4),
    })?;

    Ok(())
}

fn seed_default_thresholds(db: &Database) -> Result<()> {
    let thresholds = vec![
        // Engine Load (0x04) — 0-100%
        DefaultThreshold {
            pid_code: 0x04,
            min_value: 0.0,
            max_value: 100.0,
            low_warning: None,
            high_warning: Some(85.0),
            low_critical: None,
            high_critical: Some(95.0),
            unit: "%".to_string(),
        },
        // Coolant Temp (0x05) — -40 to 215°C
        DefaultThreshold {
            pid_code: 0x05,
            min_value: -40.0,
            max_value: 215.0,
            low_warning: Some(-10.0),
            high_warning: Some(105.0),
            low_critical: Some(-30.0),
            high_critical: Some(115.0),
            unit: "°C".to_string(),
        },
        // Short Term Fuel Trim Bank 1 (0x06) — -100 to 99.2%
        DefaultThreshold {
            pid_code: 0x06,
            min_value: -100.0,
            max_value: 99.2,
            low_warning: Some(-20.0),
            high_warning: Some(20.0),
            low_critical: Some(-30.0),
            high_critical: Some(30.0),
            unit: "%".to_string(),
        },
        // Long Term Fuel Trim Bank 1 (0x07) — -100 to 99.2%
        DefaultThreshold {
            pid_code: 0x07,
            min_value: -100.0,
            max_value: 99.2,
            low_warning: Some(-15.0),
            high_warning: Some(15.0),
            low_critical: Some(-25.0),
            high_critical: Some(25.0),
            unit: "%".to_string(),
        },
        // Short Term Fuel Trim Bank 2 (0x08) — -100 to 99.2%
        DefaultThreshold {
            pid_code: 0x08,
            min_value: -100.0,
            max_value: 99.2,
            low_warning: Some(-20.0),
            high_warning: Some(20.0),
            low_critical: Some(-30.0),
            high_critical: Some(30.0),
            unit: "%".to_string(),
        },
        // Long Term Fuel Trim Bank 2 (0x09) — -100 to 99.2%
        DefaultThreshold {
            pid_code: 0x09,
            min_value: -100.0,
            max_value: 99.2,
            low_warning: Some(-15.0),
            high_warning: Some(15.0),
            low_critical: Some(-25.0),
            high_critical: Some(25.0),
            unit: "%".to_string(),
        },
        // Fuel Pressure (0x0A) — 0-765 kPa
        DefaultThreshold {
            pid_code: 0x0A,
            min_value: 0.0,
            max_value: 765.0,
            low_warning: Some(150.0),
            high_warning: Some(700.0),
            low_critical: Some(100.0),
            high_critical: Some(750.0),
            unit: "kPa".to_string(),
        },
        // Intake MAP (0x0B) — 0-255 kPa
        DefaultThreshold {
            pid_code: 0x0B,
            min_value: 0.0,
            max_value: 255.0,
            low_warning: Some(15.0),
            high_warning: Some(240.0),
            low_critical: Some(10.0),
            high_critical: Some(250.0),
            unit: "kPa".to_string(),
        },
        // Engine RPM (0x0C) — 0-16383 rpm
        DefaultThreshold {
            pid_code: 0x0C,
            min_value: 0.0,
            max_value: 16383.0,
            low_warning: Some(500.0),
            high_warning: Some(5500.0),
            low_critical: Some(300.0),
            high_critical: Some(6500.0),
            unit: "rpm".to_string(),
        },
        // Vehicle Speed (0x0D) — 0-255 km/h
        DefaultThreshold {
            pid_code: 0x0D,
            min_value: 0.0,
            max_value: 255.0,
            low_warning: None,
            high_warning: Some(180.0),
            low_critical: None,
            high_critical: Some(220.0),
            unit: "km/h".to_string(),
        },
        // Intake Air Temp (0x0F) — -40 to 215°C
        DefaultThreshold {
            pid_code: 0x0F,
            min_value: -40.0,
            max_value: 215.0,
            low_warning: Some(-20.0),
            high_warning: Some(60.0),
            low_critical: Some(-35.0),
            high_critical: Some(80.0),
            unit: "°C".to_string(),
        },
        // MAF (0x10) — 0-655.35 g/s
        DefaultThreshold {
            pid_code: 0x10,
            min_value: 0.0,
            max_value: 655.35,
            low_warning: None,
            high_warning: Some(500.0),
            low_critical: None,
            high_critical: Some(600.0),
            unit: "g/s".to_string(),
        },
        // Throttle Position (0x11) — 0-100%
        DefaultThreshold {
            pid_code: 0x11,
            min_value: 0.0,
            max_value: 100.0,
            low_warning: None,
            high_warning: None,
            low_critical: None,
            high_critical: None,
            unit: "%".to_string(),
        },
        // Fuel Tank Level (0x2F) — 0-100%
        DefaultThreshold {
            pid_code: 0x2F,
            min_value: 0.0,
            max_value: 100.0,
            low_warning: Some(15.0),
            high_warning: None,
            low_critical: Some(5.0),
            high_critical: None,
            unit: "%".to_string(),
        },
        // Barometric Pressure (0x33) — 0-255 kPa
        DefaultThreshold {
            pid_code: 0x33,
            min_value: 0.0,
            max_value: 255.0,
            low_warning: Some(80.0),
            high_warning: None,
            low_critical: Some(70.0),
            high_critical: None,
            unit: "kPa".to_string(),
        },
        // Catalyst Temp B1S1 (0x3C) — -40 to 6513.5°C
        DefaultThreshold {
            pid_code: 0x3C,
            min_value: -40.0,
            max_value: 6513.5,
            low_warning: None,
            high_warning: Some(800.0),
            low_critical: None,
            high_critical: Some(950.0),
            unit: "°C".to_string(),
        },
        // Catalyst Temp B2S1 (0x3D)
        DefaultThreshold {
            pid_code: 0x3D,
            min_value: -40.0,
            max_value: 6513.5,
            low_warning: None,
            high_warning: Some(800.0),
            low_critical: None,
            high_critical: Some(950.0),
            unit: "°C".to_string(),
        },
        // Catalyst Temp B1S2 (0x3E)
        DefaultThreshold {
            pid_code: 0x3E,
            min_value: -40.0,
            max_value: 6513.5,
            low_warning: None,
            high_warning: Some(800.0),
            low_critical: None,
            high_critical: Some(950.0),
            unit: "°C".to_string(),
        },
        // Catalyst Temp B2S2 (0x3F)
        DefaultThreshold {
            pid_code: 0x3F,
            min_value: -40.0,
            max_value: 6513.5,
            low_warning: None,
            high_warning: Some(800.0),
            low_critical: None,
            high_critical: Some(950.0),
            unit: "°C".to_string(),
        },
        // Control Module Voltage (0x42) — 0-65.535V
        DefaultThreshold {
            pid_code: 0x42,
            min_value: 0.0,
            max_value: 65.535,
            low_warning: Some(11.5),
            high_warning: Some(15.0),
            low_critical: Some(10.5),
            high_critical: Some(16.0),
            unit: "V".to_string(),
        },
        // Ambient Air Temp (0x46) — -40 to 215°C
        DefaultThreshold {
            pid_code: 0x46,
            min_value: -40.0,
            max_value: 215.0,
            low_warning: Some(-30.0),
            high_warning: Some(50.0),
            low_critical: Some(-40.0),
            high_critical: Some(60.0),
            unit: "°C".to_string(),
        },
        // Engine Oil Temp (0x5C) — -40 to 210°C
        DefaultThreshold {
            pid_code: 0x5C,
            min_value: -40.0,
            max_value: 210.0,
            low_warning: Some(-10.0),
            high_warning: Some(120.0),
            low_critical: Some(-30.0),
            high_critical: Some(140.0),
            unit: "°C".to_string(),
        },
        // Engine Fuel Rate (0x5E) — 0-3276.75 L/h
        DefaultThreshold {
            pid_code: 0x5E,
            min_value: 0.0,
            max_value: 3276.75,
            low_warning: None,
            high_warning: Some(50.0),
            low_critical: None,
            high_critical: Some(80.0),
            unit: "L/h".to_string(),
        },
        // Transmission Temp (0xFE) — -40 to 215°C (custom/extended)
        DefaultThreshold {
            pid_code: 0xFE,
            min_value: -40.0,
            max_value: 215.0,
            low_warning: Some(-10.0),
            high_warning: Some(120.0),
            low_critical: Some(-30.0),
            high_critical: Some(135.0),
            unit: "°C".to_string(),
        },
        // Oil Pressure (0xFD) — 0-1000 kPa (custom/extended)
        DefaultThreshold {
            pid_code: 0xFD,
            min_value: 0.0,
            max_value: 1000.0,
            low_warning: Some(150.0),
            high_warning: Some(550.0),
            low_critical: Some(100.0),
            high_critical: Some(650.0),
            unit: "kPa".to_string(),
        },
        // Timing Advance (0x0E) — -64 to 63.5°
        DefaultThreshold {
            pid_code: 0x0E,
            min_value: -64.0,
            max_value: 63.5,
            low_warning: None,
            high_warning: None,
            low_critical: None,
            high_critical: None,
            unit: "°".to_string(),
        },
        // Fuel Rail Gauge Pressure (0x23) — 0-655350 kPa
        DefaultThreshold {
            pid_code: 0x23,
            min_value: 0.0,
            max_value: 655350.0,
            low_warning: None,
            high_warning: None,
            low_critical: None,
            high_critical: None,
            unit: "kPa".to_string(),
        },
        // Fuel Rail Abs Pressure (0x59) — 0-655350 kPa
        DefaultThreshold {
            pid_code: 0x59,
            min_value: 0.0,
            max_value: 655350.0,
            low_warning: None,
            high_warning: None,
            low_critical: None,
            high_critical: None,
            unit: "kPa".to_string(),
        },
        // Commanded EGR (0x2C) — 0-100%
        DefaultThreshold {
            pid_code: 0x2C,
            min_value: 0.0,
            max_value: 100.0,
            low_warning: None,
            high_warning: None,
            low_critical: None,
            high_critical: None,
            unit: "%".to_string(),
        },
        // Absolute Load (0x43) — 0-25700%
        DefaultThreshold {
            pid_code: 0x43,
            min_value: 0.0,
            max_value: 25700.0,
            low_warning: None,
            high_warning: None,
            low_critical: None,
            high_critical: None,
            unit: "%".to_string(),
        },
        // Commanded Equiv Ratio (0x44) — 0-2 λ
        DefaultThreshold {
            pid_code: 0x44,
            min_value: 0.0,
            max_value: 2.0,
            low_warning: None,
            high_warning: None,
            low_critical: None,
            high_critical: None,
            unit: "λ".to_string(),
        },
        // Demanded Torque (0x61) — -125 to 130%
        DefaultThreshold {
            pid_code: 0x61,
            min_value: -125.0,
            max_value: 130.0,
            low_warning: None,
            high_warning: None,
            low_critical: None,
            high_critical: None,
            unit: "%".to_string(),
        },
        // Actual Torque (0x62) — -125 to 130%
        DefaultThreshold {
            pid_code: 0x62,
            min_value: -125.0,
            max_value: 130.0,
            low_warning: None,
            high_warning: None,
            low_critical: None,
            high_critical: None,
            unit: "%".to_string(),
        },
        // Reference Torque (0x63) — 0-65535 Nm
        DefaultThreshold {
            pid_code: 0x63,
            min_value: 0.0,
            max_value: 65535.0,
            low_warning: None,
            high_warning: None,
            low_critical: None,
            high_critical: None,
            unit: "Nm".to_string(),
        },
    ];

    for t in &thresholds {
        db.upsert_default_threshold(t)?;
    }
    Ok(())
}

fn seed_engine_family_overrides(db: &Database) -> Result<()> {
    // W11B16 (Mini Cooper S) — higher redline, tighter coolant
    let w11_overrides = [
        PidThreshold {
            scope_type: "engine_family".to_string(),
            scope_id: "W11B16".to_string(),
            pid_code: 0x0C, // RPM
            min_value: None,
            max_value: None,
            low_warning: Some(550.0),
            high_warning: Some(6200.0),
            low_critical: Some(400.0),
            high_critical: Some(6800.0),
            notes: Some("W11B16 redline 6800 RPM".to_string()),
        },
        PidThreshold {
            scope_type: "engine_family".to_string(),
            scope_id: "W11B16".to_string(),
            pid_code: 0x05, // Coolant temp
            min_value: None,
            max_value: None,
            low_warning: Some(-5.0),
            high_warning: Some(100.0),
            low_critical: Some(-20.0),
            high_critical: Some(110.0),
            notes: Some("W11B16 tighter coolant range".to_string()),
        },
    ];

    // LLY (Duramax) — much lower redline, diesel specifics
    let lly_overrides = [
        PidThreshold {
            scope_type: "engine_family".to_string(),
            scope_id: "LLY".to_string(),
            pid_code: 0x0C, // RPM
            min_value: None,
            max_value: None,
            low_warning: Some(450.0),
            high_warning: Some(2800.0),
            high_critical: Some(3200.0),
            low_critical: Some(300.0),
            notes: Some("LLY redline 3200 RPM".to_string()),
        },
        PidThreshold {
            scope_type: "engine_family".to_string(),
            scope_id: "LLY".to_string(),
            pid_code: 0x05, // Coolant temp
            min_value: None,
            max_value: None,
            low_warning: Some(-5.0),
            high_warning: Some(98.0),
            low_critical: Some(-20.0),
            high_critical: Some(108.0),
            notes: Some("LLY diesel coolant range".to_string()),
        },
    ];

    // LFV (Malibu 1.5L Turbo) — turbo engines run hotter
    let lfv_overrides = [
        PidThreshold {
            scope_type: "engine_family".to_string(),
            scope_id: "LFV".to_string(),
            pid_code: 0x0C, // RPM
            min_value: None,
            max_value: None,
            low_warning: Some(500.0),
            high_warning: Some(5800.0),
            low_critical: Some(350.0),
            high_critical: Some(6500.0),
            notes: Some("LFV redline ~6500 RPM".to_string()),
        },
        PidThreshold {
            scope_type: "engine_family".to_string(),
            scope_id: "LFV".to_string(),
            pid_code: 0x05, // Coolant temp
            min_value: None,
            max_value: None,
            low_warning: Some(-5.0),
            high_warning: Some(105.0),
            low_critical: Some(-20.0),
            high_critical: Some(115.0),
            notes: Some("LFV turbo coolant range".to_string()),
        },
    ];

    // LSY (Malibu 2.0L Turbo) — turbo engines run hotter
    let lsy_overrides = [
        PidThreshold {
            scope_type: "engine_family".to_string(),
            scope_id: "LSY".to_string(),
            pid_code: 0x0C, // RPM
            min_value: None,
            max_value: None,
            low_warning: Some(500.0),
            high_warning: Some(5800.0),
            low_critical: Some(350.0),
            high_critical: Some(6500.0),
            notes: Some("LSY redline ~6500 RPM".to_string()),
        },
        PidThreshold {
            scope_type: "engine_family".to_string(),
            scope_id: "LSY".to_string(),
            pid_code: 0x05, // Coolant temp
            min_value: None,
            max_value: None,
            low_warning: Some(-5.0),
            high_warning: Some(105.0),
            low_critical: Some(-20.0),
            high_critical: Some(115.0),
            notes: Some("LSY turbo coolant range".to_string()),
        },
    ];

    // F23A1 (Honda Accord 2.3L) — naturally aspirated, moderate redline
    let f23a1_overrides = [
        PidThreshold {
            scope_type: "engine_family".to_string(),
            scope_id: "F23A1".to_string(),
            pid_code: 0x0C, // RPM
            min_value: None,
            max_value: None,
            low_warning: Some(500.0),
            high_warning: Some(5500.0),
            low_critical: Some(350.0),
            high_critical: Some(6100.0),
            notes: Some("F23A1 redline 6100 RPM".to_string()),
        },
        PidThreshold {
            scope_type: "engine_family".to_string(),
            scope_id: "F23A1".to_string(),
            pid_code: 0x05, // Coolant temp
            min_value: None,
            max_value: None,
            low_warning: Some(-5.0),
            high_warning: Some(102.0),
            low_critical: Some(-20.0),
            high_critical: Some(112.0),
            notes: Some("F23A1 NA coolant range".to_string()),
        },
    ];

    for t in w11_overrides
        .iter()
        .chain(lly_overrides.iter())
        .chain(lfv_overrides.iter())
        .chain(lsy_overrides.iter())
        .chain(f23a1_overrides.iter())
    {
        db.upsert_pid_threshold(t)?;
    }
    Ok(())
}

impl Database {
    /// Look up engine family id by family code.
    pub(crate) fn get_engine_family_code_id(&self, family_code: &str) -> Result<Option<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM engine_families WHERE family_code = ?1")?;
        let result = stmt.query_row(rusqlite::params![family_code], |row| row.get::<_, i64>(0));
        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
