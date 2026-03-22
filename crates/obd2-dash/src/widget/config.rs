use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{WidgetKind, WidgetSize};

/// A single widget placed in a row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetSlot {
    pub kind: WidgetKind,
    pub size: WidgetSize,
}

/// How a row's height is determined.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RowHeight {
    /// Fixed number of terminal rows.
    Fixed(u16),
    /// Minimum height — can grow to fill remaining space.
    Min(u16),
    /// Proportional weight relative to other proportional rows.
    Proportional(u16),
}

/// A row of widgets in the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetRow {
    pub widgets: Vec<WidgetSlot>,
    pub height: RowHeight,
}

/// The full dashboard configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    pub rows: Vec<WidgetRow>,
    pub version: u32,
}

impl DashboardConfig {
    /// Returns the default layout matching the current hardcoded 6-panel grid exactly.
    pub fn default_layout() -> Self {
        DashboardConfig {
            version: 1,
            rows: vec![
                // Row 0: Gauges+Engine (60%) | Temperatures (40%) — Min(10)
                WidgetRow {
                    widgets: vec![
                        WidgetSlot {
                            kind: WidgetKind::GaugesAndEngine,
                            size: WidgetSize::Half,
                        },
                        WidgetSlot {
                            kind: WidgetKind::TemperaturesPanel,
                            size: WidgetSize::Half,
                        },
                    ],
                    height: RowHeight::Min(10),
                },
                // Row 1: FuelSystem | SystemInfo | DTCs | Alerts — Fixed(12)
                WidgetRow {
                    widgets: vec![
                        WidgetSlot {
                            kind: WidgetKind::FuelSystemPanel,
                            size: WidgetSize::Half,
                        },
                        WidgetSlot {
                            kind: WidgetKind::SystemInfoPanel,
                            size: WidgetSize::Half,
                        },
                        WidgetSlot {
                            kind: WidgetKind::DtcPanel,
                            size: WidgetSize::Half,
                        },
                        WidgetSlot {
                            kind: WidgetKind::AlertsPanel,
                            size: WidgetSize::Half,
                        },
                    ],
                    height: RowHeight::Fixed(12),
                },
                // Row 2: FuelEconomy (full width) — Fixed(10)
                WidgetRow {
                    widgets: vec![WidgetSlot {
                        kind: WidgetKind::FuelEconomyPanel,
                        size: WidgetSize::Full,
                    }],
                    height: RowHeight::Fixed(10),
                },
            ],
        }
    }

    /// Load config from a JSON file, falling back to default if missing or invalid.
    pub fn load(path: &Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(contents) => match serde_json::from_str::<DashboardConfig>(&contents) {
                    Ok(config) => {
                        tracing::info!("Loaded dashboard config from {}", path.display());
                        return config;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Invalid dashboard config at {}: {}, using default",
                            path.display(),
                            e
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "Could not read dashboard config at {}: {}, using default",
                        path.display(),
                        e
                    );
                }
            }
        }
        Self::default_layout()
    }

    /// Save config to a JSON file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        tracing::info!("Saved dashboard config to {}", path.display());
        Ok(())
    }

    /// Total number of widgets across all rows.
    pub fn widget_count(&self) -> usize {
        self.rows.iter().map(|r| r.widgets.len()).sum()
    }

    /// Convert a flat widget index to (row, col) coordinates.
    pub fn flat_to_row_col(&self, flat_idx: usize) -> Option<(usize, usize)> {
        let mut remaining = flat_idx;
        for (row_idx, row) in self.rows.iter().enumerate() {
            if remaining < row.widgets.len() {
                return Some((row_idx, remaining));
            }
            remaining -= row.widgets.len();
        }
        None
    }

    /// Convert (row, col) coordinates to a flat widget index.
    #[allow(dead_code)]
    pub fn row_col_to_flat(&self, row: usize, col: usize) -> Option<usize> {
        if row >= self.rows.len() {
            return None;
        }
        if col >= self.rows[row].widgets.len() {
            return None;
        }
        let offset: usize = self.rows[..row].iter().map(|r| r.widgets.len()).sum();
        Some(offset + col)
    }

    /// Get the WidgetKind at a flat index.
    pub fn widget_at(&self, flat_idx: usize) -> Option<&WidgetSlot> {
        let (row, col) = self.flat_to_row_col(flat_idx)?;
        self.rows.get(row)?.widgets.get(col)
    }

    /// Percentage widths for widgets in a given row, considering focus.
    pub fn row_widget_percentages(&self, row_idx: usize, focused_col: Option<usize>) -> Vec<u16> {
        let row = match self.rows.get(row_idx) {
            Some(r) => r,
            None => return vec![],
        };
        let n = row.widgets.len();
        if n == 0 {
            return vec![];
        }
        if n == 1 {
            return vec![100];
        }

        match focused_col {
            Some(fc) if fc < n => {
                // Focused widget gets expanded
                let expand = if n == 2 { 70 } else { 50 };
                let remaining = 100 - expand;
                let shrunk = remaining / (n as u16 - 1);
                (0..n)
                    .map(|i| if i == fc { expand } else { shrunk })
                    .collect()
            }
            _ => {
                // Default proportions based on the original layout
                match (row_idx, n) {
                    (0, 2) => vec![60, 40],
                    (1, 4) => vec![27, 27, 23, 23],
                    (1, 3) => vec![35, 35, 30],
                    _ => {
                        let each = 100 / n as u16;
                        let mut pcts: Vec<u16> = vec![each; n];
                        // Give remainder to last widget
                        let total: u16 = pcts.iter().sum();
                        if total < 100 {
                            if let Some(last) = pcts.last_mut() {
                                *last += 100 - total;
                            }
                        }
                        pcts
                    }
                }
            }
        }
    }
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self::default_layout()
    }
}
