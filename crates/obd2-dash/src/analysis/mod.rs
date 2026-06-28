//! Real-time vehicle data analysis.
//!
//! - [`fuel_economy`]: Dual fuel economy calculation (ECU gold-standard and speed-density).
//! - [`driving`]: Driving behavior scoring — smoothness, hard brakes, jackrabbit starts.

pub mod driving;
pub mod fuel_economy;
