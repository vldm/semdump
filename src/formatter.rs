use std::num::NonZero;

use crate::{DataPart, Ref};

mod annotated;
mod color;
pub use annotated::*;
pub use color::*;

/// Reference index is 1-based index of reference in the part, used for formatting and legend.
pub type RefenceIndex = NonZero<usize>;

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
        line_size: usize,
        references_starting_offset: RefenceIndex,
        references: &[Ref],
    ) -> Result<(), Self::Error> {
        format_whole_line_inner(
            self,
            starting_offset,
            part_offset,
            bytes,
            line_size,
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

    /// Add gap after hex chunk if needed.
    fn add_hex_gap(&mut self, empty_bytes: NonZero<usize>) -> Result<(), Self::Error>;
    /// Print the ASCII representation of the bytes chunk.
    fn print_ascii_chunk(
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
    fn add_hex_gap(&mut self, empty: NonZero<usize>) -> Result<(), Self::Error> {
        (*self).add_hex_gap(empty)
    }
    fn print_ascii_chunk(
        &mut self,
        line_offset: usize,
        bytes: &[u8],
        reference: Option<FormatRef<'_>>,
    ) -> Result<(), Self::Error> {
        (*self).print_ascii_chunk(line_offset, bytes, reference)
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

type Item<'a> = (usize, &'a [u8], Option<FormatRef<'a>>);

fn inner_chunker<E>(
    part_offset: usize,
    bytes: &[u8],
    references_starting_offset: RefenceIndex,
    references: &[Ref],
    mut handler: impl FnMut(Item<'_>) -> Result<(), E>,
) -> Result<(), E> {
    let mut cursor = 0;
    for (index, reference) in references.iter().enumerate() {
        let index = references_starting_offset.saturating_add(index);
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
            "References should be sorted and non-overlapping, but reference {index} starts at {reference_start} which is before the current cursor {cursor}",
        );
        // No chunk to format before the reference, so we can directly format the reference chunk.
        if cursor < reference_start {
            handler((cursor, &bytes[cursor..reference_start], None))?;
            cursor = reference_start;
        }

        let end = reference_end.min(bytes.len());
        handler((
            cursor,
            &bytes[cursor..end],
            Some(FormatRef {
                reference,
                part_index: index,
                wrap_type,
            }),
        ))?;
        cursor = reference_end;
    }
    // Format the remaining chunk after the last reference, if any.
    if cursor < bytes.len() {
        handler((cursor, &bytes[cursor..], None))?;
    }
    Ok(())
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
    line_size: usize,
    references_starting_offset: RefenceIndex,
    references: &[Ref],
) -> Result<(), F::Error>
where
    F: Formatter + ?Sized,
{
    formatter.print_offset(starting_offset)?;
    inner_chunker(
        part_offset,
        bytes,
        references_starting_offset,
        references,
        |(line_offset, chunk_bytes, reference)| {
            formatter.print_hex_chunk(line_offset, chunk_bytes, reference)
        },
    )?;

    let gap = (line_size - (bytes.len() % line_size)) % line_size;
    if let Some(non_zero_gap) = NonZero::new(gap) {
        formatter.add_hex_gap(non_zero_gap)?;
    }

    inner_chunker(
        part_offset,
        bytes,
        references_starting_offset,
        references,
        |(line_offset, chunk_bytes, reference)| {
            formatter.print_ascii_chunk(line_offset, chunk_bytes, reference)
        },
    )?;
    formatter.flush_line()?;

    Ok(())
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
