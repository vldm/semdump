use std::iter::repeat_n;

use crate::{DataPart, Ref};

pub type RefenceIndex = usize;

/// Implementation of custom formatter for hexdump output.
pub trait Formatter {
    type Error;

    ///
    /// Whole line formatting routine.
    ///
    /// If one need to peek on the whole line before formatting,
    /// they can implement this method and call the default
    /// implementation of `format_whole_line_inner`
    /// to get the default behavior of formatting the line by calling `format_offset` and `format_hex_chunk` for each chunk.
    ///
    fn format_whole_line(
        &mut self,
        starting_offset: usize,
        part_offset: usize,
        bytes: &[u8],
        references_starting_offset: RefenceIndex,
        references: &[Ref],
    ) -> Result<(), Self::Error> {
        format_whole_line_inner(
            self,
            starting_offset,
            part_offset,
            bytes,
            references_starting_offset,
            references,
        )
    }

    fn print_part_header(&mut self, data_part: &DataPart) -> Result<(), Self::Error> {
        let _ = data_part;
        Ok(())
    }
    /// Format the offset of the current line in the hexdump.
    /// The offset should contain gap or any necessary padding to align the hex output.
    ///
    /// E.g. `00000000  `.
    ///
    fn print_offset(&mut self, offset: usize) -> Result<(), Self::Error>;
    ///
    /// Format a chunk of line of the hexdump, given the bytes and their corresponding references.
    /// The `line_offset` parameter indicates the offset within the current line of first byte in the chunk.
    /// The `bytes` parameter contains the raw bytes chunk to be formatted in hexadecimal.
    /// The `reference` parameter is an optional tuple containing the reference index and the reference itself,
    /// which can be used to apply specific formatting based on the reference information.
    ///
    /// E.g. '1A2B 3C4D' with color if reference is present.
    ///
    /// Note: Range is reference is relative to the start of the part.
    fn print_hex_chunk(
        &mut self,
        line_offset: usize,
        bytes: &[u8],
        reference: Option<FormatRef<'_>>,
    ) -> Result<(), Self::Error>;

    /// Move to the next line in the hexdump output.
    fn flush_line(&mut self) -> Result<(), Self::Error>;

    // Legend formatting routines

    /// Format the header of the legend section, which describes the references used in the hexdump.
    fn legend_header(&mut self, data_part: &str, refs_count: usize) -> Result<(), Self::Error>;

    /// Format a single entry in the legend, given a reference.
    ///
    /// The `index` parameter indicates the position of the reference in the legend, starting from 1.
    fn legend_entry(&mut self, reference: &Ref, index: RefenceIndex) -> Result<(), Self::Error>;
}

impl<F> Formatter for &mut F
where
    F: Formatter + ?Sized,
{
    type Error = F::Error;

    fn print_offset(&mut self, offset: usize) -> Result<(), Self::Error> {
        (*self).print_offset(offset)
    }

    fn print_hex_chunk(
        &mut self,
        offset: usize,
        bytes: &[u8],
        reference: Option<FormatRef<'_>>,
    ) -> Result<(), Self::Error> {
        (*self).print_hex_chunk(offset, bytes, reference)
    }

    fn flush_line(&mut self) -> Result<(), Self::Error> {
        (*self).flush_line()
    }

    fn legend_header(&mut self, data_part: &str, refs_count: usize) -> Result<(), Self::Error> {
        (*self).legend_header(data_part, refs_count)
    }

    fn legend_entry(&mut self, reference: &Ref, index: RefenceIndex) -> Result<(), Self::Error> {
        (*self).legend_entry(reference, index)
    }
}

/// Default implementation of the whole line formatting routine, which can be used by implementors of `Formatter`.
///
/// # Panics
/// If references are not sorted and non-overlapping, this function will panic with an assertion error.
pub fn format_whole_line_inner<F>(
    formatter: &mut F,
    starting_offset: usize,
    part_offset: usize,
    bytes: &[u8],
    references_starting_offset: RefenceIndex,
    references: &[Ref],
) -> Result<(), F::Error>
where
    F: Formatter + ?Sized,
{
    formatter.print_offset(starting_offset)?;
    let mut cursor = 0;

    for (index, reference) in references.iter().enumerate() {
        // reference range might start before the current line.
        let reference_start = reference.range.start.saturating_sub(part_offset);
        let wrap_type = if reference.range.start < part_offset {
            RefWrap::End
        } else if reference.range.end > part_offset + bytes.len() {
            RefWrap::Start
        } else {
            RefWrap::Single
        };
        let reference_end = reference.range.end - part_offset;

        assert!(
            reference_start >= cursor,
            "References should be sorted and non-overlapping, but reference {} starts at {reference_start} which is before the current cursor {cursor}",
            index + references_starting_offset
        );
        // No chunk to format before the reference, so we can directly format the reference chunk.
        if cursor < reference_start {
            formatter.print_hex_chunk(cursor, &bytes[cursor..reference_start], None)?;
            cursor = reference_start;
        }

        let end = reference_end.min(bytes.len());
        formatter.print_hex_chunk(
            cursor,
            &bytes[cursor..end],
            Some(FormatRef {
                reference,
                part_index: references_starting_offset + index,
                wrap_type,
            }),
        )?;
        cursor = reference_end;
    }
    // Format the remaining chunk after the last reference, if any.
    if cursor < bytes.len() {
        formatter.print_hex_chunk(cursor, &bytes[cursor..], None)?;
    }

    formatter.flush_line()?;

    Ok(())
}

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
    pub(crate) fn palette(&self, index: usize) -> Option<(usize, usize)> {
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

    fn flush_line(&mut self) -> Result<(), Self::Error> {
        writeln!(self.writer)?;
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
pub fn palette(i: usize) -> (usize, usize) {
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
    let (fg, bg) = PAIRS[i % PAIRS.len()];
    (fg, bg)
}

/// Type of reference wrap
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefWrap {
    /// Reference starts on the current line and continues on the next line(s).
    Start,
    /// Reference ends on the current line, but started on the previous line(s).
    End,
    /// Reference is fully contained within the current line.
    Single,
}

/// Reference information for used in [`Formatter`] trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatRef<'a> {
    pub reference: &'a Ref,
    /// Index of reference in the part.
    pub part_index: RefenceIndex,
    /// Marker indicating that this
    /// reference started on line before and continues on the current line.
    pub wrap_type: RefWrap,
}

///
/// Formatter that add reference after hexdump line.
///
/// Reference is added in the form of ASCII art using characters like '└', '─', '┘', `┴` to indicate the span of the reference.
/// The starting and ending symbols are either point to byte in range or a formatting gap between bytes.
///
/// Example:
/// ```text
/// 00000000|  1A2B 3C4D 5E6F 7A8B 9C0D E1F2 3456 7890
///         |  └[1]┘       └[2]┘        └[3]┴[4]┘└1┘
/// ```

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotateFormatter<W>
where
    W: std::io::Write,
{
    writer: W,
    annotation_line: Vec<char>,
    // Indicates whether part need to be separated in order to fit annotations.
    // need_new_separator: bool,
}

impl<W> AnnotateFormatter<W>
where
    W: std::io::Write,
{
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            annotation_line: Vec::new(),
        }
    }

    /// Create a buffer for annotation span.
    ///
    fn create_annotation_buffer(
        &mut self,
        gap_before: bool,
        mut span_len: usize,
        ref_wrap: RefWrap,
    ) -> &mut [char] {
        let mut borrowed_gap = None;
        if gap_before {
            span_len += 1;
            borrowed_gap = self.annotation_line.pop();
        }
        let start = self.annotation_line.len();
        self.annotation_line.extend(repeat_n('-', span_len));

        let span = &mut self.annotation_line[start..];
        // add start symbol
        if matches!(ref_wrap, RefWrap::Start | RefWrap::Single) {
            if borrowed_gap == Some('┘') {
                span[0] = '┴';
            } else {
                span[0] = '└';
            }
        }
        // add end symbol
        if matches!(ref_wrap, RefWrap::End | RefWrap::Single) {
            span[span_len - 1] = '┘';
        }
        span
    }
}

impl<W> Formatter for AnnotateFormatter<W>
where
    W: std::io::Write,
{
    type Error = std::io::Error;

    fn print_offset(&mut self, offset: usize) -> Result<(), Self::Error> {
        write!(self.writer, "{offset:08X}| ")
    }

    fn print_hex_chunk(
        &mut self,
        line_offset: usize,
        bytes: &[u8],
        reference: Option<FormatRef<'_>>,
    ) -> Result<(), Self::Error> {
        // Print hex as is
        for (i, &byte) in bytes.iter().enumerate() {
            let gap_before = (line_offset + i).is_multiple_of(2) && (line_offset + i) != 0;
            if gap_before {
                write!(self.writer, " ")?;
            }
            // we print hex as pair of bytes
            write!(self.writer, "{byte:02X}")?;
        }

        let have_gap_before = line_offset.is_multiple_of(2) && line_offset != 0;
        let have_gap_after = (line_offset + bytes.len()).is_multiple_of(2);
        let num_gaps_between = (bytes.len().saturating_sub(1)) / 2;
        let span_len = bytes.len() * 2 + num_gaps_between + usize::from(have_gap_after);

        // Add annotation of reference index in format of:
        // └─IDX─┘
        if let Some(ref reference) = reference {
            // idx is guaranteed to be 1 or 2 digits long.
            let idx_str = if reference.reference.range.end - reference.reference.range.start > 1 {
                format!("[{}]", reference.part_index)
            } else {
                format!("{}", reference.part_index)
            };
            let id_len = idx_str.chars().count();

            let span =
                self.create_annotation_buffer(have_gap_before, span_len, reference.wrap_type);

            let span_len = span.len();

            let idx_pos = (span_len - id_len) / 2;

            // Can't format id in the span, just fill it with '='
            if id_len > span_len {
                span.fill('=');
            }

            // copy idx_str to annotation_buffer at idx_pos
            for (i, c) in idx_str.chars().enumerate() {
                span[idx_pos + i] = c;
            }
        } else {
            // If no reference, just add spaces to keep annotation line aligned with hex output.
            self.annotation_line
                .extend(std::iter::repeat_n(' ', span_len));
        }

        Ok(())
    }

    fn flush_line(&mut self) -> Result<(), Self::Error> {
        writeln!(self.writer)?;
        if !self.annotation_line.is_empty() {
            let annotation_str: String = self.annotation_line.iter().collect();
            writeln!(self.writer, "        | {annotation_str}",)?;
        }
        self.annotation_line.clear();
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
        writeln!(
            self.writer,
            "  [{index:>2}] {}   range=0x{:04X}..0x{:04X} ({})",
            reference.name,
            reference.range.start,
            reference.range.end,
            reference.range.end - reference.range.start
        )?;
        Ok(())
    }
}
