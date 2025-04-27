// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by Rachel Mant <git@dragonmux.network>

use color_eyre::eyre::Result;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Alignment, Margin, Rect, Size};
use ratatui::symbols::scrollbar;
use ratatui::widgets::{Block, BorderType, Padding, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget};
use ratatui::DefaultTerminal;
use ratatui::Frame;


pub struct Viewer<'a>
{
    exit: bool,
    title: &'a str,
    docs: &'a str,

    viewport_size: Size,
    line_count: usize,
    max_scroll: usize,
    scroll_position: usize,
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
        // Work out how any lines the documentation renders to
        let line_count = tui_markdown::from_str(docs).lines.len();

        Self {
            exit: false,
            title: title.as_str(),
            docs: docs.as_str(),
            viewport_size,
            line_count,
            // Compute the maximum scrolling position for the scrollbar
            max_scroll: line_count.saturating_sub(viewport_size.height.into()),
            scroll_position: 0,
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
                        KeyCode::Up => self.scroll_up(),
                        KeyCode::Down => self.scroll_down(),
                        _ => {},
                    }
                }
            },
            Event::Resize(width, height) => self.handle_resize(width, height),
            _ => {},
        }
        Ok(())
    }

    fn quit(&mut self)
    {
        self.exit = true
    }

    fn handle_resize(&mut self, width: u16, height: u16)
    {
        // Grab the new viewport size and store that
        self.viewport_size = Size::new(width, height);
        // Figure out if the scroll position is still viable, and adjust it appropriately
        let max_scroll = self.line_count.saturating_sub(height.into());
        if self.scroll_position > max_scroll {
            self.scroll_position = max_scroll
        }
        // Update the max scroll position too
        self.max_scroll = max_scroll;
    }

    fn scroll_up(&mut self)
    {
        // Scrolling up is easy.. just keep subtracting 1 until we reach 0 and keep it at 0
        self.scroll_position = self.scroll_position.saturating_sub(1)
    }

    fn scroll_down(&mut self)
    {
        // Scrolling down is a bit harder - start by computing what the next scroll position should be
        let new_position = self.scroll_position + 1;
        // Now, if that does not exceed the actual max scroll position, we can update our scroll position
        if new_position <= self.max_scroll {
            self.scroll_position = new_position;
        }
    }
}

impl Widget for &mut Viewer<'_>
{
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized
    {
        // Convert the documentation to display from Markdown
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

        // Build the scrollbar state
        let mut scroll_state = ScrollbarState::new(self.max_scroll)
            .position(self.scroll_position);
        // Build and render the scrollbar to track the content
        StatefulWidget::render
        (
            // Put the scrollbar on the right side, running down the text, and don't display
            // the end arrows
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .symbols(scrollbar::VERTICAL)
                .begin_symbol(None)
                .end_symbol(None),
            // Scrollbar should be displayed inside the side of the block, not overwriting the corners
            area.inner(Margin::new(0, 1)),
            buf,
            &mut scroll_state
        );
    }
}
