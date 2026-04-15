use ratatui::{
    style::{Color, Style},
    text::{Line, Span, Text},
};

use crate::formatter::Formatter;

///
/// Color like formatting, but instead of emitting ANSI escape codes to terminal,
/// it uses Ratatui's styling capabilities to apply colors and styles to the hexdump output.
///
/// This formatter doesn't implement `Formatter` trait dirrectly, because it meant to be used as `&mut` reference instead,
/// this is done to ensure that buffer is consumed after hexdump is rendered.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RatatuiFormatter {
    buffer: Buffer,
    active_line: Line<'static>,
    theme: RatatuiTheme,
}

impl RatatuiFormatter {
    /// Create a new `RatatuiFormatter` with an empty buffer.
    pub fn new() -> Self {
        Self::new_with_theme(RatatuiTheme::default())
    }

    /// Create a new `RatatuiFormatter` with a custom theme.
    pub fn new_with_theme(theme: RatatuiTheme) -> Self {
        Self {
            buffer: Buffer { lines: Vec::new() },
            theme,
            active_line: Line::default(),
        }
    }

    /// Convert processed format result into Ratatui's `Text` type, which can be used for rendering in a Ratatui application.
    pub fn into_text(self) -> Text<'static> {
        Text::from(self.buffer.lines)
    }

    fn pallet_style(&self, index: super::RefenceIndex) -> Style {
        if self.theme.refs_palette.is_empty() {
            return Style::new();
        }
        self.theme.refs_palette[index.get() % self.theme.refs_palette.len()]
    }
}

impl Default for RatatuiFormatter {
    fn default() -> Self {
        Self::new()
    }
}

/// Ratatui theme for hexdump formatting.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RatatuiTheme {
    /// Style for data part header, e.g: `Data <LABEL> 0x0000..0x1000 (100bytes):`
    pub part_header_style: Style,
    /// Style for offset, e.g: `00000000`
    pub offset_style: Style,
    /// Style for hex bytes, e.g: `1A2B 3C4D 5E6F 7A8B 9C0D E1F2 3456 7890`
    pub hex_style: Style,
    /// Style for ascii representation of bytes, e.g: `|.A.B.C.D.E.F.G.H|`
    pub ascii_style: Style,
    /// Style for refs legend header, e.g: `Legend:`
    pub legend_header_style: Style,
    /// Style for refs legend entries, e.g: `[1]` in `[1] Ref description`
    pub legend_entry_style: Style,
    /// Palette of styles for references, indexed by reference index (1-based).
    ///
    /// Note: ref pallet are mixed with hex and ascii style using patch.
    pub refs_palette: Vec<Style>,
}

fn default_palette() -> Vec<Style> {
    vec![
        Style::new().white().bg(Color::Red),
        Style::new().black().bg(Color::Green),
        Style::new().white().bg(Color::Blue),
        Style::new().black().bg(Color::Yellow),
        Style::new().white().bg(Color::Magenta),
        Style::new().black().bg(Color::Cyan),
        Style::new().black().bg(Color::White),
        Style::new().white().bg(Color::LightRed),
        Style::new().black().bg(Color::LightGreen),
        Style::new().white().bg(Color::LightBlue),
        Style::new().black().bg(Color::LightYellow),
        Style::new().black().bg(Color::LightCyan),
        Style::new().white().bg(Color::LightMagenta),
    ]
}

impl Default for RatatuiTheme {
    fn default() -> Self {
        Self {
            part_header_style: Style::new().light_yellow(),
            offset_style: Style::new().light_green(),
            hex_style: Style::new(),
            ascii_style: Style::new()
                .underline_color(Color::LightMagenta)
                .underlined(),
            legend_header_style: Style::new().light_yellow(),
            legend_entry_style: Style::new(),
            refs_palette: default_palette(),
        }
    }
}

#[derive(Default, Debug, Clone, Eq, PartialEq)]
struct Buffer {
    pub lines: Vec<Line<'static>>,
}

impl Formatter for &mut RatatuiFormatter {
    type Error = std::convert::Infallible;

    fn print_part_header(
        &mut self,
        starting_offset: usize,
        data_part: &crate::DataPart,
    ) -> Result<(), Self::Error> {
        if !data_part.label.is_empty() {
            let start = starting_offset;
            let end = starting_offset + data_part.bytes.len();
            let size = end - start;
            let line = Line::from(format!(
                "--- {} (0x{start:08X}..0x{end:08X}, {} bytes) ---",
                data_part.label, size
            ));
            self.buffer.lines.push(line);
        }
        Ok(())
    }

    fn print_offset(&mut self, offset: usize) -> Result<(), Self::Error> {
        let line = Span::from(format!("{offset:08X}  "));
        self.active_line.spans.push(line);
        Ok(())
    }
    fn print_hex_chunk(
        &mut self,
        line_offset: usize,
        bytes: &[u8],
        reference: Option<super::FormatRef<'_>>,
    ) -> Result<(), Self::Error> {
        for (i, &byte) in bytes.iter().enumerate() {
            let gap_before = (line_offset + i).is_multiple_of(2);
            if gap_before {
                self.active_line.spans.push(Span::from(" "));
            }

            let style = if let Some(ref format_ref) = reference {
                let palette_style = self.pallet_style(format_ref.part_index);
                palette_style.patch(self.theme.hex_style)
            } else {
                self.theme.hex_style
            };
            self.active_line
                .spans
                .push(Span::from(format!("{byte:02X}")).style(style));
        }
        Ok(())
    }
    fn add_hex_gap(&mut self, empty_bytes: std::num::NonZero<usize>) -> Result<(), Self::Error> {
        let gap_size = (empty_bytes.get() * 2) + (empty_bytes.get() / 2);
        self.active_line
            .spans
            .push(Span::from(" ".repeat(gap_size)));
        Ok(())
    }
    fn print_ascii_chunk(
        &mut self,
        line_offset: usize,
        bytes: &[u8],
        reference: Option<super::FormatRef<'_>>,
    ) -> Result<(), Self::Error> {
        // if it start of chunk - add sparator between hex and ascii
        if line_offset == 0 {
            self.active_line.spans.push(Span::from(" |"));
        }
        let style = if let Some(ref format_ref) = reference {
            let palette_style = self.pallet_style(format_ref.part_index);
            palette_style.patch(self.theme.ascii_style)
        } else {
            self.theme.ascii_style
        };
        let ascii_str: String = bytes
            .iter()
            .map(|&b| {
                if (0x20..=0x7E).contains(&b) {
                    b as char
                } else {
                    '·' // U+00B7 middle dot — ligature-safe
                }
            })
            .collect();
        self.active_line
            .spans
            .push(Span::from(ascii_str).style(style));

        Ok(())
    }
    fn flush_line(&mut self) -> Result<(), Self::Error> {
        self.buffer
            .lines
            .push(std::mem::take(&mut self.active_line));
        Ok(())
    }
    fn legend_header(&mut self, data_part: &str, refs_count: usize) -> Result<(), Self::Error> {
        let description = if refs_count == 0 {
            "none".to_string()
        } else {
            format!(
                "{} reference{}",
                refs_count,
                if refs_count > 1 { "s" } else { "" }
            )
        };
        self.buffer.lines.push(Line::from(vec![
            Span::from(format!(" REF ({data_part}): {description}"))
                .style(self.theme.legend_header_style),
        ]));
        Ok(())
    }
    fn legend_entry(
        &mut self,
        reference: &crate::Ref,
        index: super::RefenceIndex,
    ) -> Result<(), Self::Error> {
        let ref_style = self
            .pallet_style(index)
            .patch(self.theme.legend_entry_style);

        let index_span = Span::from(format!("[{index:>2}]")).style(ref_style);
        let label_span = Span::from(format!(
            " {}   range=0x{:04X}..0x{:04X} ({})",
            reference.label,
            reference.range.start,
            reference.range.end,
            reference.range.end.saturating_sub(reference.range.start)
        ))
        .style(ref_style);
        self.buffer
            .lines
            .push(Line::from(vec![index_span, label_span]));
        Ok(())
    }
}
