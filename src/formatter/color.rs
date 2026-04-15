use std::num::NonZero;

use crate::Ref;

use super::{FormatRef, Formatter, RefenceIndex};

pub type PaletteFn = Box<dyn Fn(RefenceIndex) -> Option<(usize, usize)>>;
///
/// Formatter that adds ANSI color codes to the hex output based on the references.
///
pub struct ColorFormatter<W>
where
    W: std::io::Write,
{
    writer: W,
    custom_palette: Option<PaletteFn>,
}

impl<W> ColorFormatter<W>
where
    W: std::io::Write,
{
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            custom_palette: None,
        }
    }

    ///
    /// Create a new `ColorFormatter` with a custom palette function.
    ///
    /// Palette function takes a reference index and returns a tuple of ANSI color codes (foreground and background).
    /// Palette function can return `None` to indicate that no color should be applied for the given reference index.
    pub fn with_palette(writer: W, palette: PaletteFn) -> Self {
        Self {
            writer,
            custom_palette: Some(palette),
        }
    }

    /// Get the ANSI color codes for the given reference index.
    pub(crate) fn palette(&self, index: RefenceIndex) -> Option<(usize, usize)> {
        if let Some(ref custom_palette) = self.custom_palette {
            custom_palette(index)
        } else {
            Some(palette(index))
        }
    }
}

impl<W> Formatter for ColorFormatter<W>
where
    W: std::io::Write,
{
    type Error = std::io::Error;

    fn print_offset(&mut self, offset: usize) -> Result<(), Self::Error> {
        write!(self.writer, "{offset:08X}  ")
    }

    /// Print hex as pair of bytes (half of word) with color if reference is present.
    fn print_hex_chunk(
        &mut self,
        line_offset: usize,
        bytes: &[u8],
        reference: Option<FormatRef<'_>>,
    ) -> Result<(), Self::Error> {
        for (i, &byte) in bytes.iter().enumerate() {
            let gap_before = (line_offset + i).is_multiple_of(2);
            if gap_before {
                write!(self.writer, " ")?;
            }

            if let Some(ref format_ref) = reference
                && let Some((fg, bg)) = self.palette(format_ref.part_index)
            {
                write!(self.writer, "\x1b[{fg};{bg}m{byte:02X}\x1b[0m")?;
            } else {
                write!(self.writer, "{byte:02X}")?;
            }
        }
        Ok(())
    }
    fn add_hex_gap(&mut self, empty: NonZero<usize>) -> Result<(), Self::Error> {
        let gap = (empty.get() * 2) + (empty.get() / 2); // 2 spaces for hex + 1 space for gap every 2 bytes
        write!(self.writer, "{:width$}", " ", width = gap)?;
        Ok(())
    }
    fn print_ascii_chunk(
        &mut self,
        line_offset: usize,
        bytes: &[u8],
        reference: Option<FormatRef<'_>>,
    ) -> Result<(), Self::Error> {
        if line_offset == 0 {
            write!(self.writer, " |")?;
        }
        for &b in bytes {
            let ch = if (0x20..=0x7E).contains(&b) {
                b as char
            } else {
                '·' // U+00B7 middle dot — ligature-safe
            };

            if let Some(ref format_ref) = reference
                && let Some((fg, bg)) = self.palette(format_ref.part_index)
            {
                write!(self.writer, "\x1b[{fg};{bg}m{ch}\x1b[0m")?;
            } else {
                write!(self.writer, "{ch}")?;
            }
        }
        Ok(())
    }

    fn flush_line(&mut self) -> Result<(), Self::Error> {
        writeln!(self.writer, "|")?;
        Ok(())
    }

    fn legend_header(&mut self, data_part: &str, refs_count: usize) -> Result<(), Self::Error> {
        writeln!(
            self.writer,
            "  REFS ({}): {}",
            data_part,
            if refs_count == 0 {
                "none".to_string()
            } else {
                format!(
                    "{} reference{}",
                    refs_count,
                    if refs_count > 1 { "s" } else { "" }
                )
            }
        )?;
        Ok(())
    }

    fn legend_entry(&mut self, reference: &Ref, index: RefenceIndex) -> Result<(), Self::Error> {
        if let Some((fg, bg)) = self.palette(index) {
            writeln!(
                self.writer,
                "  [\x1b[{fg};{bg}m{index:>2}\x1b[0m] {}   range=0x{:04X}..0x{:04X} ({})",
                reference.name,
                reference.range.start,
                reference.range.end,
                reference.range.end.saturating_sub(reference.range.start)
            )?;
        } else {
            writeln!(
                self.writer,
                "  [{index:>2}] {}   range=0x{:04X}..0x{:04X} ({})",
                reference.name,
                reference.range.start,
                reference.range.end,
                reference.range.end.saturating_sub(reference.range.start)
            )?;
        }
        Ok(())
    }
}

/// Palette function with no color.
pub fn no_color_palette(_: RefenceIndex) -> Option<(usize, usize)> {
    None
}

/// Returns (`fg_code`, `bg_code`) — high-contrast ANSI pairs, excluding default white-on-black.
///
/// Default palette used in `ColorFormatter` if no custom palette is provided.
pub fn palette(i: RefenceIndex) -> (usize, usize) {
    // Hand-picked (fg, bg) pairs: every combination has strong contrast,
    // avoids default terminal colors (white on black), and adjacent indices
    // use distinct hues for easy visual discrimination.
    const PAIRS: &[(usize, usize)] = &[
        (97, 41),  // bright white on red
        (30, 42),  // black on green
        (97, 44),  // bright white on blue
        (30, 43),  // black on yellow
        (97, 45),  // bright white on magenta
        (30, 46),  // black on cyan
        (30, 47),  // black on white
        (97, 101), // bright white on bright red
        (30, 102), // black on bright green
        (97, 104), // bright white on bright blue
        (30, 103), // black on bright yellow
        (30, 106), // black on bright cyan
        (97, 105), // bright white on bright magenta
    ];
    let (fg, bg) = PAIRS[i.get() % PAIRS.len()];
    (fg, bg)
}
