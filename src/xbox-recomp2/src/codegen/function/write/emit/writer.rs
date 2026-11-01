//! Where instruction emitters write their C++ code.
//!
//! - [`cpp!`] writes C++ lines. Prefer it in emitters.
//! - [`CodeWriter`] holds the text and the current indentation.

use core::fmt;

/// Writes C++ lines into a [`CodeWriter`].
/// Like `format!` with `indoc`.
///
/// # Examples
///
/// ```text
/// cpp!(ctx.out, "
///     while (count_{base} > 0) {{
///         count_{base}--;
///     }}
/// ");
/// ```
///
/// # Remarks
///
/// - Write C++ braces as `{{` and `}}`.
/// - Each 4 leading spaces after unindenting add one tab to the depth.
/// - The last line always ends with a newline.
macro_rules! cpp {
    ($out:expr, $($fmt:tt)+) => {{
        use ::core::fmt::Write as _;
        let out: &mut $crate::codegen::function::write::emit::CodeWriter = &mut *$out;
        // Writing into a `String` can't fail.
        let _ = ::indoc::writedoc!(out, $($fmt)+);
        out.end_line();
    }};
}

pub(super) use cpp;

/// Collects one instruction's emitted C++ text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::codegen::function) struct CodeWriter {
    text: String,
    depth: usize,
    at_line_start: bool,
}

impl CodeWriter {
    /// Creates an empty writer at function-body depth.
    pub fn new() -> Self {
        Self {
            text: String::new(),
            depth: 1,
            at_line_start: true,
        }
    }

    /// Removes the text and resets the depth, keeping the storage.
    pub fn clear(&mut self) {
        self.text.clear();
        self.depth = 1;
        self.at_line_start = true;
    }

    /// Returns the text written so far.
    ///
    /// # Returns
    ///
    /// Everything written since [`Self::new`] or [`Self::clear`].
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Indents the following lines one more level.
    pub fn indent(&mut self) -> &mut Self {
        self.depth += 1;
        self
    }

    /// Indents the following lines one less level.
    pub fn dedent(&mut self) -> &mut Self {
        self.depth -= 1;
        self
    }

    /// Writes `text` as one complete line, exactly as given.
    ///
    /// # Arguments
    ///
    /// * `text` - the line, without a trailing newline.
    pub fn line(&mut self, text: &str) -> &mut Self {
        self.start_line(0);
        self.text.push_str(text);
        self.newline();
        self
    }

    /// Appends complete lines exactly as given, without indenting them.
    ///
    /// Use this for fixed templates that carry their own indentation.
    ///
    /// # Arguments
    ///
    /// * `lines` - the text to append, including its newlines.
    pub fn raw(&mut self, lines: &str) -> &mut Self {
        self.text.push_str(lines);
        self
    }

    /// Ends the current line unless it is already empty.
    pub fn end_line(&mut self) {
        if !self.at_line_start {
            self.newline();
        }
    }

    /// Ends the current line.
    fn newline(&mut self) {
        self.text.push('\n');
        self.at_line_start = true;
    }

    /// Writes the indentation, plus `levels` tabs, on the first append of a line.
    fn start_line(&mut self, levels: usize) {
        if self.at_line_start {
            self.at_line_start = false;
            for _ in 0..self.depth + levels {
                self.text.push('\t');
            }
        }
    }
}

/// Lets [`cpp!`] write straight into the buffer.
///
/// Each full group of 4 leading spaces on a line becomes one tab. Any
/// spaces left over stay as written.
impl fmt::Write for CodeWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for (index, part) in s.split('\n').enumerate() {
            // Every piece after the first starts a new line.
            if index > 0 {
                self.newline();
            }
            if part.is_empty() {
                continue;
            }

            // Turn leading spaces into extra indentation.
            let mut body = part;
            if self.at_line_start {
                let spaces = part.len() - part.trim_start_matches(' ').len();
                let tabs = spaces / 4;
                body = &part[tabs * 4..];
                self.start_line(tabs);
            }
            self.text.push_str(body);
        }
        Ok(())
    }
}

impl Default for CodeWriter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ── Indentation ──────────────────────────────────────────────────────────

    /// Full groups of 4 spaces become tabs; leftover spaces stay as written.
    #[rstest]
    #[case::literal_block("    a;\n        b;\n", "\t\ta;\n\t\t\tb;\n")]
    #[case::leftover_spaces(" a;\n      b;\n", "\t a;\n\t\t  b;\n")]
    #[case::multiline_value_keeps_continuation("(0 +\n  1)", "\t(0 +\n\t  1)")]
    fn writer_should_turn_leading_spaces_into_tabs(#[case] input: &str, #[case] expected: &str) {
        // Arrange.
        let mut writer = CodeWriter::new();

        // Act.
        fmt::Write::write_str(&mut writer, input).unwrap();

        // Assert.
        assert_eq!(writer.as_str(), expected);
    }
}
