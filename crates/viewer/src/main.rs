use std::io;

use battery_monitor_core::load_config;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use sled::Db;
use tui::{
    backend::CrosstermBackend,
    widgets::{Block, Borders, Paragraph},
    Terminal,
};

fn read_latest_capacity(db: &Db) -> Option<f64> {
    db.iter()
        .values()
        .rev()
        .next()
        .and_then(|res| res.ok())
        .map(|ivec| {
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&ivec);
            f64::from_be_bytes(arr)
        })
}

fn main() -> anyhow::Result<()> {
    let config = load_config(None)?;
    let db = sled::open(&config.database_path)?;

    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| {
            let size = f.size();
            let capacity = read_latest_capacity(&db)
                .map(|c| format!("{c:.2}%"))
                .unwrap_or_else(|| "N/A".into());
            let block = Paragraph::new(capacity)
                .block(Block::default().title("Battery").borders(Borders::ALL));
            f.render_widget(block, size);
        })?;

        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    terminal::disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
