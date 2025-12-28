mod event;
mod tui;

use std::{
    fs::{File, OpenOptions},
    io::{Read, Write, stdout},
    ops::{Index, IndexMut, Range},
    path::PathBuf,
};

use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::tui::{
    Formatting, Measurement,
    status::StatusComponent,
    text::TextComponent,
    tree::{ComponentNode, ComponentTree, Frame, LayoutMode},
};

fn main() -> Result<()> {
    let mut args = std::env::args();
    let _program_name = args.next();
    let file_name = args.next();
    // Set up file logging (logs to reovim.log)
    let log_file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open("reovim.log")?;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with_writer(log_file)
        .with_ansi(false)
        .init();

    info!("reovim starting");

    // Enter alternate screen buffer and enable raw mode
    terminal::enable_raw_mode()?;
    stdout()
        .execute(EnterAlternateScreen)?
        .execute(EnableMouseCapture)?;

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
    stdout()
        .execute(LeaveAlternateScreen)?
        .execute(DisableMouseCapture)?;
    terminal::disable_raw_mode()?;

    info!("reovim shutting down");
    result
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
    file_path: Option<PathBuf>,
    contents: String,
    dimensions: (u16, u16),
    render_range: Range<usize>,
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            contents: String::new(),
            dimensions: Default::default(),
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
            contents,
            file_path: Some(path.clone()),
            ..Default::default()
        })
    }

    fn run(&mut self) -> Result<()> {
        let mut stdout = stdout();
        self.dimensions = crossterm::terminal::size()?;
        self.render_range = 0..self.dimensions.1.saturating_sub(2) as usize;

        let file_name = self
            .file_path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|os_string| os_string.to_str())
            .unwrap_or("[no file]");

        let root_frame = Frame::new(LayoutMode::VerticalSplit);
        let mut tree = ComponentTree::new(tui::tree::ComponentNode::Frame(root_frame));
        let editor_frame = Frame::new(LayoutMode::HorizontalSplit);
        let editor_formatting = Formatting {
            preferred_width: Measurement::Percent(100),
            preferred_height: Measurement::Fill, // Leave room for status line
            ..Formatting::default()
        };
        let editor_frame_id = tree.add_child_with_formatting(
            0,
            tui::tree::ComponentNode::Frame(editor_frame),
            editor_formatting,
        )?;
        let text = TextComponent::new(&self.contents, self.dimensions.0);
        tree.add_child(editor_frame_id, ComponentNode::Text(text))?;

        let status_line = StatusComponent::new(file_name);
        tree.add_child(0, ComponentNode::Status(status_line))?;

        loop {
            self.dimensions = crossterm::terminal::size()?;
            tree.layout(self.dimensions.0, self.dimensions.1);
            tree.render(&mut stdout)?;
            stdout.flush()?;

            let crossterm_event = crossterm::event::read().expect("failed to read event");
            // nowe we handle them events
            match crossterm_event {
                crossterm::event::Event::FocusGained => {}
                crossterm::event::Event::FocusLost => {}
                crossterm::event::Event::Key(key_event) => {
                    tree.update(event::ReovimEvent::Key(key_event))?;
                    if key_event.code.is_esc() {
                        break;
                    }
                }
                crossterm::event::Event::Mouse(mouse_event) => {
                    tree.update(event::ReovimEvent::Mouse(mouse_event))?
                }
                crossterm::event::Event::Paste(_) => {}
                crossterm::event::Event::Resize(x, y) => {
                    tree.update(event::ReovimEvent::Resize(x, y))?
                }
            }
        }
        Ok(())
    }
}
