use ratatui::style::Color;
use crate::config::ColorConfig;

#[derive(Clone)]
pub struct Theme {
    pub accent: Color,
    pub error: Color,
    pub background: Color,
}

impl Theme {
    pub fn from_config(cfg: &ColorConfig) -> Self {
        fn parse_color(s: &str) -> Color {
            match s.to_lowercase().as_str() {
                "black" => Color::Black,
                "white" => Color::White,
                "red" => Color::Red,
                "green" => Color::Green,
                "blue" => Color::Blue,
                "yellow" => Color::Yellow,
                "magenta" => Color::Magenta,
                "cyan" => Color::Cyan,
                "gray" | "grey" => Color::Gray,
                _ => Color::Reset,
            }
        }

        Theme {
            accent: cfg
                .accent
                .as_deref()
                .map(parse_color)
                .unwrap_or(Color::Cyan),
            error: cfg
                .error
                .as_deref()
                .map(parse_color)
                .unwrap_or(Color::Red),
            background: cfg
                .background
                .as_deref()
                .map(parse_color)
                .unwrap_or(Color::Black),
        }
    }
}
