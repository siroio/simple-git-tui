use std::{
    io::{self, Stdout},
    sync::{
        Arc,
        atomic::AtomicBool,
        mpsc::{Receiver, Sender},
    },
    time::Duration,
};

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{config::Config, theme::Theme};

mod view;
mod view_model;

pub use view_model::{UiMessage, ViewModel};

pub struct App {
    view_model: ViewModel,
}

impl App {
    pub fn new(
        config: Config,
        theme: Theme,
        tx: Sender<UiMessage>,
        rx: Receiver<UiMessage>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Self {
        Self {
            view_model: ViewModel::new(config, theme, tx, rx, cancel_flag),
        }
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        let res = self.event_loop(&mut terminal);
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        res
    }

    fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> anyhow::Result<()> {
        loop {
            self.view_model.poll_messages();
            if self.view_model.take_full_redraw() {
                terminal.clear()?;
            }
            terminal.draw(|f| view::draw(&mut self.view_model, f))?;
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    let should_quit = self.view_model.handle_key(key)?;
                    if should_quit {
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}
