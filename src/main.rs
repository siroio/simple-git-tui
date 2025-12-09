mod app;
mod config;
mod git;
mod theme;

use std::sync::{
    atomic::AtomicBool,
    mpsc,
    Arc,
};

use app::{App};
use config::load_config;
use theme::Theme;

fn main() -> anyhow::Result<()> {
    let cfg = load_config()?;
    let theme = Theme::from_config(&cfg.colors);

    let (tx, rx) = mpsc::channel();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let app = App::new(cfg, theme, tx, rx, cancel_flag);

    app.run()
}
