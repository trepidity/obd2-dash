pub mod ai;
pub mod diagnostics;
pub mod driving;
pub mod fuel_economy;
pub mod mock_profile;
pub mod obd2;
pub mod recording;
pub mod state;

// Convenience re-exports
pub use driving::DrivingBehavior;
pub use fuel_economy::{FuelEconomyState, SensorSnapshot};
pub use mock_profile::MockVehicleProfile;
pub use obd2::connection_prefs::ConnectionPrefs;
pub use obd2::dtc::DTC_SCENARIO_COUNT;
pub use obd2::mock::MockObd2;
pub use obd2::scanner::{DeviceKind, DiscoveredDevice, ScanEvent};
pub use obd2::vin;
pub use obd2::{AdapterInfo, Dtc, Obd2Connection, Obd2Error, Pid, PidReading, VehicleData};
pub use recording::storage::{StorageConfig, StorageManager};
pub use recording::RecordingState;
pub use state::{ConnectionState, DomainMessage, DomainState, SpeedUnit, TemperatureUnit};
