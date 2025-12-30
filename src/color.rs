use supports_color::Stream;

/// Detect if the terminal supports ANSI colors
pub fn supports_colors() -> bool {
    supports_color::on(Stream::Stdout).is_some()
}

/// Detect the level of color support
pub fn color_level() -> ColorLevel {
    match supports_color::on(Stream::Stdout) {
        Some(level) => match level.has_16m {
            true => ColorLevel::TrueColor,
            false => match level.has_256 {
                true => ColorLevel::Color256,
                false => ColorLevel::Color16,
            },
        },
        None => ColorLevel::None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorLevel {
    /// No color support
    None,
    /// 16 basic colors
    Color16,
    /// 256 ANSI colors
    Color256,
    /// True color (24-bit RGB)
    TrueColor,
}
