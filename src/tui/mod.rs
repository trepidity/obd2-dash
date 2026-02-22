pub mod event;
pub mod panel;
pub mod ui;

use std::io::{self, stdout};

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

pub type Terminal = ratatui::Terminal<CrosstermBackend<io::Stdout>>;

/// Manages terminal state — enters alternate screen on creation, restores on drop.
pub struct Tui {
    pub terminal: Terminal,
}

impl Tui {
    pub fn new() -> anyhow::Result<Self> {
        let terminal = ratatui::Terminal::new(CrosstermBackend::new(stdout()))?;
        Ok(Self { terminal })
    }

    pub fn enter(&mut self) -> anyhow::Result<()> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen)?;
        self.terminal.hide_cursor()?;
        self.terminal.clear()?;

        // Install panic hook that restores terminal
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            original_hook(panic_info);
        }));

        Ok(())
    }

    pub fn exit(&mut self) -> anyhow::Result<()> {
        self.terminal.show_cursor()?;
        disable_raw_mode()?;
        execute!(stdout(), LeaveAlternateScreen)?;
        Ok(())
    }

    pub fn draw(&mut self, state: &crate::app::AppState, log_buffer: &crate::debug_log::LogBuffer) -> anyhow::Result<()> {
        self.terminal.draw(|frame| ui::render(frame, state, log_buffer))?;
        Ok(())
    }
}
