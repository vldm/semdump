//!
//! Library for rendering hexdump with additional semantic information.
//! - Add spans to hexdump in order to visualize range of bytes corresponding to a specific reference;
//! - Fully zero copy;
//! - Allow customization of output formats (e.g. enhance with colors, split dump into smaller ranges etc.);
//!
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
use std::{borrow::Cow, num::NonZero, ops::Range};

pub use crate::formatter::ColorFormatter;
use crate::formatter::Formatter;

pub mod formatter;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FormatConfig {
    color: bool,
}

impl FormatConfig {
    pub fn new() -> Self {
        Self { color: true }
    }
    fn set_color(&mut self, color: bool) -> &mut Self {
        self.color = color;
        self
    }
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self::new()
    }
}

type StartingOffset = usize;

///
/// Main entrypoint of semantic dump.
///
/// 1. It provides interface for building the dump by pushing parts of data and references to them.
/// 2. It provides interface for rendering the dump using custom `Formatter` implementation.
///
/// ## Example:
/// ```
/// use semdump::{SemanticDump, DataPart, ColorFormatter};
///
/// let mut dump = SemanticDump::new(0);
///
///
/// // The data part can have local reference
/// let mut last_span = DataPart::from_bytes(vec![0xAA, 0xBB, 0xCC, 0xDD]);
/// last_span.push_ref(0..4, "some other data");
///
/// dump.push_part(DataPart::from_bytes(vec![0x01, 0x02, 0x03, 0x04]))
///     // or one can use global references
///     .add_global_ref(0..2, "first two bytes")
///     .add_global_ref(2..4, "last two bytes")
///     .push_gap(16)
///     .push_part(last_span);
///
/// let formatter = ColorFormatter::new(std::io::stdout());
/// dump.render(formatter).unwrap();
///
/// ```
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct SemanticDump<'src> {
    parts: Vec<(StartingOffset, GapOrPart<'src>)>,
    va_start: StartingOffset,
    config: FormatConfig,
}

impl<'src> SemanticDump<'src> {
    ///
    /// Creates a new empty [`SemanticDump`].
    ///
    /// `starting_offset` is the initial offset for the first byte in the dump.
    #[must_use]
    pub fn new(starting_offset: usize) -> Self {
        Self {
            parts: Vec::new(),
            va_start: starting_offset,
            config: FormatConfig::default(),
        }
    }

    fn push_inner(&mut self, part: GapOrPart<'src>) {
        let offset = self
            .parts
            .last()
            .map_or(self.va_start, |(off, part)| match part {
                GapOrPart::Gap(gap_size) => off + gap_size,
                GapOrPart::Part(data_part) => off + data_part.bytes.len(),
            });

        self.parts.push((offset, part));
    }

    /// Sets whether to use ANSI color code for highlighting references in the hexdump.
    pub fn set_color(&mut self, color: bool) -> &mut Self {
        self.config.set_color(color);
        self
    }
    ///
    /// Pushes a new section of data with its associated references.
    /// Checkout [`DataPart`] for details.
    ///
    /// ## Note: all parts are rendered sequentially,
    /// so the offset of each part is started right after the previous one.
    pub fn push_part(&mut self, part: DataPart<'src>) -> &mut Self {
        self.push_inner(GapOrPart::Part(part));
        self
    }
    /// Returns the current offset in the dump, which is the offset of the next byte to be added.
    pub fn offset(&self) -> usize {
        self.parts
            .last()
            .map_or(self.va_start, |(off, part)| match part {
                GapOrPart::Gap(gap_size) => off + gap_size,
                GapOrPart::Part(data_part) => off + data_part.bytes.len(),
            })
    }

    ///
    /// Add a gap of `num_bytes` zero bytes to the dump.
    /// Useful for visual separation of different sections.
    ///
    /// This gap will not be rendered in the hexdump.
    /// References to this gap are not allowed.
    ///
    /// ## Note: all parts are rendered sequentially,
    /// so the offset of each part is started right after the previous one.
    pub fn push_gap(&mut self, num_bytes: usize) -> &mut Self {
        self.push_inner(GapOrPart::Gap(num_bytes));
        self
    }

    ///
    /// Add a reference to a specific range of bytes in the dump.
    /// This will effectively find corresponding [`DataPart`] containing this range and add reference to it.
    ///
    /// `range` should be valid within some specific already insterted `DataPart`.
    ///
    /// # Panics
    /// if `range` is out of bounds of any existing `DataPart`.
    pub fn add_global_ref(&mut self, range: Range<usize>, name: impl Into<String>) -> &mut Self {
        for (part_start, part) in &mut self.parts {
            if let GapOrPart::Part(data_part) = part {
                let part_end = *part_start + data_part.bytes.len();
                if range.start >= *part_start && range.end <= part_end {
                    // Found the part containing the reference range
                    let local_range = (range.start - *part_start)..(range.end - *part_start);
                    data_part.push_ref(local_range, name);
                    return self;
                }
            }
        }
        panic!("Reference range {range:?} is out of bounds of any existing DataPart");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GapOrPart<'a> {
    Gap(usize),
    Part(DataPart<'a>),
}

///
/// Represents a contiguous section of bytes with associated references to external entities.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPart<'a> {
    pub bytes: Cow<'a, [u8]>,
    pub refs: Vec<Ref>,
}

impl<'a> DataPart<'a> {
    /// Creates a new [`DataPart`] from the given bytes, with empty list of references.
    pub fn from_bytes(bytes: impl Into<Cow<'a, [u8]>>) -> Self {
        Self {
            bytes: bytes.into(),
            refs: Vec::new(),
        }
    }

    ///
    /// Mark specific region of bytes as a reference.
    ///
    /// `range` should be valid within the `bytes` of this part.
    ///
    /// # Panics
    /// if `range` is out of bounds.
    pub fn push_ref(&mut self, range: Range<usize>, name: impl Into<String>) -> &mut Self {
        assert!(
            range.start <= range.end && range.end <= self.bytes.len(),
            "Reference range {range:?} is out of bounds of DataPart bytes (len={})",
            self.bytes.len()
        );
        self.refs.push(Ref::new(range, name));

        self.refs.sort_by_key(|r| r.range.start);
        debug_assert!(
            self.refs
                .windows(2)
                .all(|w| w[0].range.end <= w[1].range.start),
            "References should not overlap after sorting, but found overlapping references: {:#?}",
            self.refs
        );
        self
    }
}

/// Span of bytes in the `DataPart` with specific label and optinal extra information index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// Range of bytes in the `DataPart` that this reference corresponds to.
    pub range: Range<usize>,
    /// Human readable name of the reference, used in legends and custom `Formatter` implementations.
    pub name: String,
    /// Index to external information about this reference, used in custom `Formatter` implementations.
    /// 0 by default.
    pub extra_info: usize,
}

impl Ref {
    /// Creates a new reference with the given byte range and name.
    pub fn new(range: Range<usize>, name: impl Into<String>) -> Self {
        Self::raw(range, name, 0)
    }
    /// Creates a new reference with the given byte range, name, and extra information index.
    pub fn raw(range: Range<usize>, name: impl Into<String>, extra_info: usize) -> Self {
        Self {
            range,
            name: name.into(),
            extra_info,
        }
    }
}

impl SemanticDump<'_> {
    /// Renders the hexdump with references using the provided `Formatter`.
    ///
    /// The `Formatter` defines how the hexdump is formatted, including how references are highlighted and how legends are displayed.
    ///
    /// # Errors
    /// Returns an error if the `Formatter` encounters an issue during rendering.
    pub fn render<F>(&self, mut formatter: F) -> Result<(), F::Error>
    where
        F: Formatter,
    {
        for (offset, part) in &self.parts {
            match part {
                GapOrPart::Gap(_) => {
                    // Gaps are not rendered, so we can skip them.
                }
                GapOrPart::Part(data_part) => {
                    render_part(&mut formatter, *offset, data_part)?;
                }
            }
        }
        Ok(())
    }
}

/// Renders a hexdump of a `DataPart` with optional color highlighting for references.
/// `part_base` - initial offset of the part in the overall data segment.
/// color - whether to use ANSI color codes for highlighting.
/// part - the `DataPart` to render.
fn render_part<F>(mut out: F, part_base: usize, part: &DataPart) -> Result<(), F::Error>
where
    F: Formatter,
{
    // Collect bytes into a vector for indexing
    let bytes = &part.bytes;

    let refs = &part.refs[..];
    let mut start_index = 0;

    let cols = 16;

    for (line_idx, chunk) in bytes.chunks(cols).enumerate() {
        let part_offset = line_idx * cols;

        let mut next_index = start_index;
        for r in &refs[start_index..] {
            if r.range.start > part_offset + chunk.len() {
                break;
            }
            next_index += 1;
        }

        let references = &refs[start_index..next_index];

        let starting_offset = part_base + part_offset;
        out.format_whole_line(
            starting_offset,
            part_offset,
            chunk,
            cols,
            NonZero::new(1 + start_index).unwrap(),
            references,
        )?;

        // Update start_index to skip already rendered references.
        // Last reference may be partially used in both lines, so check and update starting_index accordingly.
        let last_index_wrap = next_index
            .checked_sub(1)
            .is_some_and(|i| refs[i].range.end > part_offset + chunk.len());
        if last_index_wrap {
            start_index = next_index - 1;
        } else {
            start_index = next_index;
        }
        // } else {
        //     start_index = next_index + 1;
        // }
    }

    out.legend_header("test", part.refs.len())?;
    for (index, reference) in part.refs.iter().enumerate() {
        let index = NonZero::new(1 + index).unwrap();
        out.legend_entry(reference, index)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::formatter::{AnnotateFormatter, ColorFormatter, no_color_palette};

    use super::*;

    fn test_data() -> DataPart<'static> {
        DataPart {
            bytes: vec![
                0x01, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0xDE, 0xAD,
                0xBE, 0xEF, 0x48, 0x65, 0x6C, 0x6C, 0x6F, 0x00, 0x00, 0x00, 0x34, 0x12, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22, 0x33, 0x44,
                0x55, 0x66, 0x77, 0x88, 0x99, 0x00, 0x00, 0x00, 0x00, 0x00, 0xAA, 0xBB, 0xCC, 0xDD,
                0xEE, 0xFF, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0x00,
            ]
            .into(),
            refs: vec![
                Ref {
                    range: 0..6,
                    name: ".Lanon.faeb22a22ed4190fdf8d8c764500d80d.47".into(),
                    extra_info: 0,
                },
                Ref {
                    range: 8..21,
                    name: "very_very_long_human_readable_field_name".into(),
                    extra_info: 0,
                },
                Ref {
                    range: 24..27,
                    name: ".Lanon.a91b73428f0e239f7d2e4cbd3eaa0011.02".into(),
                    extra_info: 0,
                },
                Ref {
                    range: 46..47,
                    name: "tiny".into(),
                    extra_info: 0,
                },
            ],
        }
    }

    // impl of stdout that locks stdout for the whole lifetime
    struct MyStdout {
        stdout: std::io::StdoutLock<'static>,
    }
    fn stdout() -> MyStdout {
        MyStdout {
            stdout: std::io::stdout().lock(),
        }
    }

    impl std::io::Write for MyStdout {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.stdout.write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.stdout.flush()
        }
    }

    #[test]
    fn can_use_stdout() {
        let part = test_data();
        let formatter = ColorFormatter::new(stdout());

        render_part(formatter, 0, &part).unwrap();
    }

    #[test]
    fn snapshot_color_example() {
        let part = test_data();

        let mut vec = Vec::new();

        let no_color_formatter =
            ColorFormatter::with_palette(Cursor::new(&mut vec), Box::new(no_color_palette));
        render_part(no_color_formatter, 0, &part).unwrap();
        let output = String::from_utf8(vec).unwrap();
        insta::assert_snapshot!(output);
    }

    #[test]
    fn snapshot_annotate() {
        let part = test_data();

        let mut vec = Vec::new();

        let annotate_formatter = AnnotateFormatter::new(Cursor::new(&mut vec));
        render_part(annotate_formatter, 0, &part).unwrap();
        let output = String::from_utf8(vec).unwrap();
        insta::assert_snapshot!(output);
    }

    #[test]
    fn manual_compare_no_color() {
        let part = test_data();

        let mut vec = Vec::new();

        let no_color_formatter =
            ColorFormatter::with_palette(Cursor::new(&mut vec), Box::new(no_color_palette));
        render_part(no_color_formatter, 0, &part).unwrap();
        let output = String::from_utf8(vec).unwrap();

        assert_eq!(
            output,
            "00000000   0100 0000 2000 0000 1000 0000 DEAD BEEF |···· ···········|
00000010   4865 6C6C 6F00 0000 3412 0000 0000 0000 |Hello···4·······|
00000020   AABB CCDD EEFF 1122 3344 5566 7788 9900 |·······\"3DUfw···|
00000030   0000 0000 AABB CCDD EEFF 1122 3344 5566 |···········\"3DUf|
00000040   7788 9900                               |w···|
  REFS (test): 4 references
  [ 1] .Lanon.faeb22a22ed4190fdf8d8c764500d80d.47   range=0x0000..0x0006 (6)
  [ 2] very_very_long_human_readable_field_name   range=0x0008..0x0015 (13)
  [ 3] .Lanon.a91b73428f0e239f7d2e4cbd3eaa0011.02   range=0x0018..0x001B (3)
  [ 4] tiny   range=0x002E..0x002F (1)
"
        );
    }

    #[test]
    fn manual_compare_annotate() {
        let part = test_data();

        let mut vec = Vec::new();

        let no_color_formatter = AnnotateFormatter::new(Cursor::new(&mut vec));
        render_part(no_color_formatter, 0, &part).unwrap();
        let output = String::from_utf8(vec).unwrap();

        println!("OUTPUT:\n{output}");
        assert_eq!(
            output,
            r#"00000000| 0100 0000 2000 0000 1000 0000 DEAD BEEF |···· ···········|
        | └-----[1]-----┘    └--------[2]---------|                |
00000010| 4865 6C6C 6F00 0000 3412 0000 0000 0000 |Hello···4·······|
        | ----[2]----┘       └-[3]--┘             |                |
00000020| AABB CCDD EEFF 1122 3344 5566 7788 9900 |·······"3DUfw···|
        |                                   └4┘   |                |
00000030| 0000 0000 AABB CCDD EEFF 1122 3344 5566 |···········"3DUf|
        |                                         |                |
00000040| 7788 9900                               |w···|
        |                                         |    |
  REFS (test): 4 references
  [ 1] .Lanon.faeb22a22ed4190fdf8d8c764500d80d.47   range=0x0000..0x0006 (6)
  [ 2] very_very_long_human_readable_field_name   range=0x0008..0x0015 (13)
  [ 3] .Lanon.a91b73428f0e239f7d2e4cbd3eaa0011.02   range=0x0018..0x001B (3)
  [ 4] tiny   range=0x002E..0x002F (1)
"#
        );
    }

    #[test]
    fn test_multi_part() {
        let mut dump = SemanticDump::new(0);
        dump.push_part(DataPart::from_bytes(vec![0x01, 0x02, 0x03, 0x04]))
            .add_global_ref(0..2, "first two bytes")
            .add_global_ref(2..4, "last two bytes");
        dump.push_gap(16);

        let dump_offset = dump.offset();

        dump.push_part(DataPart::from_bytes(vec![0xAA, 0xBB, 0xCC, 0xDD]))
            .add_global_ref(dump_offset..dump_offset + 4, "some other data");

        let formatter = ColorFormatter::new(stdout());
        dump.render(formatter).unwrap();
    }
}
