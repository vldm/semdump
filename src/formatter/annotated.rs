use std::{iter::repeat_n, num::NonZero};

use crate::{DataPart, Ref};

use super::{FormatRef, Formatter, RefWrap, RefenceIndex};

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

    fn print_part_header(
        &mut self,
        starting_offset: usize,
        data_part: &DataPart,
    ) -> Result<(), Self::Error> {
        if !data_part.label.is_empty() {
            let start = starting_offset;
            let end = starting_offset + data_part.bytes.len();
            let size = end - start;
            writeln!(
                self.writer,
                "--- {} (0x{start:08X}..0x{end:08X}, {} bytes) ---",
                data_part.label, size
            )?;
        }
        Ok(())
    }

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
    fn add_hex_gap(&mut self, empty_bytes: NonZero<usize>) -> Result<(), Self::Error> {
        let gap = (empty_bytes.get() * 2) + (empty_bytes.get() / 2); // 2 spaces for hex + 1 space for gap every 2 bytes

        write!(self.writer, "{:width$}", " ", width = gap)?;
        self.annotation_line.extend(std::iter::repeat_n(' ', gap)); // Add space to annotation line for the gap

        Ok(())
    }

    fn print_ascii_chunk(
        &mut self,
        line_offset: usize,
        bytes: &[u8],
        _reference: Option<FormatRef<'_>>,
    ) -> Result<(), Self::Error> {
        if line_offset == 0 {
            write!(self.writer, " |")?;
            self.annotation_line.push('|');
        }
        for &b in bytes {
            let ch = if (0x20..=0x7E).contains(&b) {
                b as char
            } else {
                '·' // U+00B7 middle dot — ligature-safe
            };
            write!(self.writer, "{ch}")?;
        }
        self.annotation_line
            .extend(std::iter::repeat_n(' ', bytes.len()));
        Ok(())
    }

    fn flush_line(&mut self) -> Result<(), Self::Error> {
        writeln!(self.writer, "|")?;
        self.annotation_line.push('|');
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
            reference.label,
            reference.range.start,
            reference.range.end,
            reference.range.end - reference.range.start
        )?;
        Ok(())
    }
}
