use super::config::{DashboardConfig, RowHeight, WidgetRow, WidgetSlot};
use super::{WidgetCategory, WidgetKind, WidgetSize, widget_registry};

/// Current phase of the edit mode state machine.
#[derive(Debug, Clone)]
pub enum EditPhase {
    /// Browsing widgets with a cursor, can delete or initiate add.
    Browse,
    /// Picking a category for a new widget.
    CategoryPicker { selected: usize },
    /// Picking a widget type within a category.
    WidgetPicker {
        category: WidgetCategory,
        selected: usize,
    },
    /// Picking a size for the new widget.
    SizePicker {
        kind: WidgetKind,
        selected: usize,
    },
}

/// Full state for edit mode.
#[derive(Debug, Clone)]
pub struct EditModeState {
    /// Working copy of the config being edited.
    pub working_config: DashboardConfig,
    /// Current cursor position (flat widget index, or row count if appending).
    pub cursor: usize,
    /// Current phase.
    pub phase: EditPhase,
}

impl EditModeState {
    pub fn new(config: &DashboardConfig) -> Self {
        Self {
            working_config: config.clone(),
            cursor: 0,
            phase: EditPhase::Browse,
        }
    }

    /// Move cursor to next widget in Browse mode.
    pub fn cursor_next(&mut self) {
        let count = self.working_config.widget_count();
        if count > 0 {
            self.cursor = (self.cursor + 1) % count;
        }
    }

    /// Move cursor to previous widget in Browse mode.
    pub fn cursor_prev(&mut self) {
        let count = self.working_config.widget_count();
        if count > 0 {
            if self.cursor == 0 {
                self.cursor = count - 1;
            } else {
                self.cursor -= 1;
            }
        }
    }

    /// Delete the widget at the current cursor position.
    pub fn delete_at_cursor(&mut self) {
        if let Some((row, col)) = self.working_config.flat_to_row_col(self.cursor) {
            let row_widgets = &mut self.working_config.rows[row].widgets;
            if !row_widgets.is_empty() {
                row_widgets.remove(col);
                // Remove empty rows
                if row_widgets.is_empty() {
                    self.working_config.rows.remove(row);
                }
                // Adjust cursor
                let count = self.working_config.widget_count();
                if count > 0 && self.cursor >= count {
                    self.cursor = count - 1;
                }
            }
        }
    }

    /// Start the add-widget flow.
    pub fn start_add(&mut self) {
        self.phase = EditPhase::CategoryPicker { selected: 0 };
    }

    /// Handle Enter in CategoryPicker — advance to WidgetPicker.
    pub fn select_category(&mut self) {
        if let EditPhase::CategoryPicker { selected } = &self.phase {
            let categories = WidgetCategory::all();
            if let Some(&cat) = categories.get(*selected) {
                self.phase = EditPhase::WidgetPicker {
                    category: cat,
                    selected: 0,
                };
            }
        }
    }

    /// Handle Enter in WidgetPicker — advance to SizePicker.
    pub fn select_widget(&mut self) {
        if let EditPhase::WidgetPicker {
            category, selected, ..
        } = &self.phase
        {
            let registry = widget_registry();
            let widgets_in_cat: Vec<_> = registry
                .iter()
                .filter(|m| m.category == *category)
                .collect();
            if let Some(meta) = widgets_in_cat.get(*selected) {
                self.phase = EditPhase::SizePicker {
                    kind: meta.kind,
                    selected: match meta.default_size {
                        WidgetSize::Half => 0,
                        WidgetSize::Full => 1,
                    },
                };
            }
        }
    }

    /// Handle Enter in SizePicker — insert the widget and go back to Browse.
    pub fn select_size(&mut self) {
        if let EditPhase::SizePicker { kind, selected } = &self.phase {
            let size = if *selected == 0 {
                WidgetSize::Half
            } else {
                WidgetSize::Full
            };
            let slot = WidgetSlot { kind: *kind, size };

            // Insert into the config: add to the last row, or create a new row
            if size == WidgetSize::Full || self.working_config.rows.is_empty() {
                self.working_config.rows.push(WidgetRow {
                    widgets: vec![slot],
                    height: RowHeight::Fixed(8),
                });
            } else {
                // Find the last row and add there if it has room (< 3 widgets)
                let last_row = self.working_config.rows.last_mut().unwrap();
                if last_row.widgets.len() < 3 {
                    last_row.widgets.push(slot);
                } else {
                    self.working_config.rows.push(WidgetRow {
                        widgets: vec![slot],
                        height: RowHeight::Fixed(8),
                    });
                }
            }

            // Move cursor to the newly added widget
            self.cursor = self.working_config.widget_count().saturating_sub(1);
            self.phase = EditPhase::Browse;
        }
    }

    /// Move selection up in a picker.
    pub fn picker_up(&mut self) {
        match &mut self.phase {
            EditPhase::CategoryPicker { selected } => {
                let count = WidgetCategory::all().len();
                if count > 0 {
                    *selected = if *selected == 0 {
                        count - 1
                    } else {
                        *selected - 1
                    };
                }
            }
            EditPhase::WidgetPicker {
                category, selected, ..
            } => {
                let count = widget_registry()
                    .iter()
                    .filter(|m| m.category == *category)
                    .count();
                if count > 0 {
                    *selected = if *selected == 0 {
                        count - 1
                    } else {
                        *selected - 1
                    };
                }
            }
            EditPhase::SizePicker { selected, .. } => {
                *selected = if *selected == 0 { 1 } else { 0 };
            }
            EditPhase::Browse => {}
        }
    }

    /// Move selection down in a picker.
    pub fn picker_down(&mut self) {
        match &mut self.phase {
            EditPhase::CategoryPicker { selected } => {
                let count = WidgetCategory::all().len();
                if count > 0 {
                    *selected = (*selected + 1) % count;
                }
            }
            EditPhase::WidgetPicker {
                category, selected, ..
            } => {
                let count = widget_registry()
                    .iter()
                    .filter(|m| m.category == *category)
                    .count();
                if count > 0 {
                    *selected = (*selected + 1) % count;
                }
            }
            EditPhase::SizePicker { selected, .. } => {
                *selected = if *selected == 0 { 1 } else { 0 };
            }
            EditPhase::Browse => {}
        }
    }

    /// Go back one phase (Esc in pickers).
    pub fn go_back(&mut self) -> bool {
        match &self.phase {
            EditPhase::Browse => false, // Esc in Browse exits edit mode
            EditPhase::CategoryPicker { .. } => {
                self.phase = EditPhase::Browse;
                true
            }
            EditPhase::WidgetPicker { .. } => {
                self.phase = EditPhase::CategoryPicker { selected: 0 };
                true
            }
            EditPhase::SizePicker { .. } => {
                self.phase = EditPhase::CategoryPicker { selected: 0 };
                true
            }
        }
    }

    /// Get category titles for the picker.
    pub fn category_items() -> Vec<(&'static str, &'static str)> {
        WidgetCategory::all()
            .iter()
            .map(|c| (c.title(), c.description()))
            .collect()
    }

    /// Get widget titles for a category picker.
    pub fn widget_items(category: WidgetCategory) -> Vec<(&'static str, &'static str)> {
        widget_registry()
            .into_iter()
            .filter(|m| m.category == category)
            .map(|m| (m.title, m.description))
            .collect()
    }
}
