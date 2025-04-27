// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use color_eyre::eyre::Result;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Alignment, Rect, Size};
use ratatui::widgets::{Block, BorderType, Padding, Widget};
use ratatui::DefaultTerminal;
use ratatui::Frame;


pub struct Viewer<'a>
{
    exit: bool,
    title: &'a str,
    docs: &'a str,

    viewport_size: Size,
}

impl<'a> Viewer<'a>
{
    pub fn display(title: &'a String, docs: &'a String) -> Result<()>
    {
        // Grab the console, putting it in TUI mode
        let mut terminal = ratatui::init();
        // Turn the Markdown to display into a viewer
        let mut viewer = Self::new(title, docs, terminal.size()?);

        // Run the viewer and wait to see what the user does
        let result = viewer.run(&mut terminal);
        // When they get done, put the console back and propagate any errors
        ratatui::restore();
        result
    }

    fn new(title: &'a String, docs: &'a String, viewport_size: Size) -> Self
    {
        Self {
            exit: false,
            title: title.as_str(),
            docs: docs.as_str(),
            viewport_size,
        }
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()>
    {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame)
    {
        frame.render_widget(self, frame.area())
    }

    fn handle_events(&mut self) -> Result<()>
    {
        match event::read()?
        {
            Event::Key(key) =>
            {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q' | 'Q') => self.quit(),
                        _ => {},
                    }
                }
            },
            Event::Resize(width, height) => self.viewport_size = Size::new(width, height),
            _ => {},
        }
        Ok(())
    }

    fn quit(&mut self)
    {
        self.exit = true
    }
}

impl Widget for &mut Viewer<'_>
{
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized
    {
        let docs_text = tui_markdown::from_str(self.docs);

        // Build a bordered block for presentation
        let block = Block::bordered()
            .title(self.title)
            .title_alignment(Alignment::Left)
            .border_type(BorderType::Rounded)
            .padding(Padding::horizontal(1));

        // Render the contents of the block (the docs text), then the block itself
        docs_text.render(block.inner(area), buf);
        block.render(area, buf);
    }
}
