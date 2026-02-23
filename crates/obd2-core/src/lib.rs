pub mod obd2;
pub mod fuel_economy;
pub mod driving;
pub mod recording;
pub mod diagnostics;
pub mod mock_profile;
pub mod state;

// Convenience re-exports
pub use obd2::{Obd2Connection, Pid, Dtc, PidReading, VehicleData, AdapterInfo, Obd2Error};
pub use obd2::mock::MockObd2;
pub use obd2::scanner::{DeviceKind, DiscoveredDevice, ScanEvent};
pub use obd2::connection_prefs::ConnectionPrefs;
pub use obd2::dtc::DTC_SCENARIO_COUNT;
pub use obd2::vin;
pub use mock_profile::MockVehicleProfile;
pub use recording::RecordingState;
pub use recording::storage::{StorageConfig, StorageManager};
pub use state::{DomainState, DomainMessage, ConnectionState, TemperatureUnit, SpeedUnit};
pub use fuel_economy::{FuelEconomyState, SensorSnapshot};
pub use driving::DrivingBehavior;
