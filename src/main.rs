mod event;
mod tui;

use std::{
    ffi::OsStr,
    fs::File,
    io::{Read, Stdout, stdout},
    ops::{Index, IndexMut, Range, RangeBounds},
    os::unix::fs::FileExt,
    path::PathBuf,
    str::FromStr,
};

use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    cursor::MoveToNextLine,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, EnterAlternateScreen, LeaveAlternateScreen},
};
use tracing::info;
use tracing_subscriber::EnvFilter;
use unicode_width::UnicodeWidthStr;

fn main() -> Result<()> {
    let mut args = std::env::args();
    let _program_name = args.next();
    let file_name = args.next();
    // Set up file logging (logs to reovim.log)
    let log_file = File::create("reovim.log")?;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with_writer(log_file)
        .with_ansi(false)
        .init();

    info!("reovim starting");

    // Enter alternate screen buffer and enable raw mode
    terminal::enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;

    let mut editor = match file_name {
        Some(file_name) => {
            info!("opening file {file_name}");
            let mut path_buf = PathBuf::new();
            path_buf.push(std::env::current_dir()?);
            let path_buf = path_buf.join(file_name);
            Buffer::from_file_path(&path_buf)?
        }
        None => Buffer::default(),
    };
    let result = editor.run();

    // Always restore terminal state, even if run() fails
    stdout().execute(LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    info!("reovim shutting down");
    result
}

struct Cursor {
    row: usize,
    col: usize,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            row: Default::default(),
            col: Default::default(),
        }
    }
}

struct Line {
    line_number: usize,
    /// Need's to be string for mutability, womp womp
    contents: String,
}

impl Default for Line {
    fn default() -> Self {
        Self {
            line_number: 0,
            contents: Default::default(),
        }
    }
}

impl Line {
    /// First we get the cols_width_between, then we access perform to action
    fn new(line_number: usize, contents: &str) -> Self {
        Self {
            line_number,
            contents: String::from(contents),
        }
    }

    /// First we get the cols_width_between, then we access perform to action
    fn cols_width_between(&self, range: Range<usize>) -> usize {
        self[range].width()
    }
}

impl IndexMut<Range<usize>> for Line {
    fn index_mut(&mut self, index: Range<usize>) -> &mut Self::Output {
        let len = self.contents.len();
        let start = index.start.min(len);
        let end = index.end.min(len).max(start); // ensure end >= start
        &mut self.contents[start..end]
    }
}

impl<'a> Index<Range<usize>> for Line {
    type Output = str;

    fn index(&self, index: Range<usize>) -> &Self::Output {
        let len = self.contents.len();
        let start = index.start.min(len);
        let end = index.end.min(len).max(start); // ensure end >= start
        &self.contents[start..end]
    }
}

pub enum Command {}

struct Buffer {
    /// If we don't have a file descriptor we just open a new buffer
    file: Option<File>,
    file_path: Option<PathBuf>,
    lines: Vec<Line>,
    commands: Vec<Command>,
    cursor: Cursor,
    dimensions: (u16, u16),
    rows_number: usize,
    render_range: Range<usize>,
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            file: Default::default(),
            lines: Default::default(),
            commands: Default::default(),
            cursor: Default::default(),
            dimensions: Default::default(),
            rows_number: Default::default(),
            render_range: Default::default(),
            file_path: Default::default(),
        }
    }
}

impl Buffer {
    fn from_file_path(path: &PathBuf) -> Result<Buffer> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(_) => File::create_new(path)?,
        };
        let mut contents = String::new();
        // todo: We can store the contents as a file buffer
        file.read_to_string(&mut contents)?;
        let mut lines = Vec::new();
        for (line_number, line) in contents.lines().enumerate() {
            lines.push(Line::new(line_number, line));
        }
        Ok(Buffer {
            file: Some(file),
            file_path: Some(path.clone()),
            rows_number: lines.len().to_string().len(),
            lines,
            ..Default::default()
        })
    }

    fn render_status_line(&self, stdout: &mut Stdout) -> Result<()> {
        let file_name = self
            .file_path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|os_str| os_str.to_str())
            .unwrap_or("buffer");
        stdout.execute(crossterm::cursor::MoveTo(0, self.dimensions.1 - 2))?;
        stdout
            .execute(SetBackgroundColor(Color::Reset))?
            .execute(SetBackgroundColor(Color::Black))?
            .execute(SetForegroundColor(Color::Yellow))?
            .execute(Print(file_name))?
            .execute(MoveToNextLine(1))?
            .execute(SetBackgroundColor(Color::Reset))?;
        Ok(())
    }

    fn render_text(&self, stdout: &mut Stdout) -> Result<()> {
        stdout.execute(crossterm::cursor::MoveTo(0, 0))?;
        for line in &self.lines[self.render_range.start..self.render_range.end] {
            info!("{} {}", line.line_number, line.contents);
            let gutter_width = self.rows_number;
            let gutter_str = format!(" {:>gutter_width$} ", line.line_number.to_string());
            stdout
                .execute(SetBackgroundColor(Color::Reset))?
                .execute(SetBackgroundColor(Color::DarkGrey))?
                .execute(Print(gutter_str))?
                .execute(SetBackgroundColor(Color::Reset))?
                .execute(Print(" "))?
                .execute(Print(line.contents.clone()))?
                .execute(MoveToNextLine(1))?;
        }
        Ok(())
    }

    fn run(&mut self) -> Result<()> {
        let mut stdout = stdout();
        loop {
            self.dimensions = crossterm::terminal::size()?;
            self.render_range = 0..self.dimensions.1.saturating_sub(2) as usize;
            info!("{:?}", self.dimensions);
            stdout
                .execute(Clear(crossterm::terminal::ClearType::All))?
                .execute(SetForegroundColor(Color::White))?;

            self.render_text(&mut stdout)?;
            self.render_status_line(&mut stdout)?;

            stdout.execute(ResetColor)?;

            // nowe we handle them events, yehaw
            if let crossterm::event::Event::Key(key) =
                crossterm::event::read().expect("failed to read event")
            {
                if key
                    .code
                    .is_modifier(crossterm::event::ModifierKeyCode::LeftMeta)
                {
                    info!("yoooo");
                }
                break;
            }
        }
        Ok(())
    }
}

struct Window {}

impl Window {}
