//! The files of a program, and where each one sits in the global offset
//! space that spans use.
//!
//! Each file is given a range of offsets starting at its `base`, with a gap
//! of one between files, so every byte of every file has a distinct offset
//! and a [`Span`](crate::Span) needs no separate file id.

/// One source file.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// The name shown in messages, like `src/motors/types.tds`.
    pub name: String,
    pub text: String,
    /// The global offset of the file's first byte.
    pub base: usize,
}

impl SourceFile {
    /// The global offset just past the file's last byte.
    pub fn end(&self) -> usize {
        self.base + self.text.len()
    }
}

/// A 1-based line and column (in characters) inside a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> SourceMap {
        SourceMap::default()
    }

    /// A map holding a single file at base 0.
    pub fn single(name: &str, text: &str) -> SourceMap {
        let mut map = SourceMap::new();
        map.add(name, text);
        map
    }

    /// Adds a file and returns its base offset.
    pub fn add(&mut self, name: &str, text: &str) -> usize {
        let base = self.files.last().map_or(0, |f| f.end() + 1);
        self.files.push(SourceFile {
            name: name.to_string(),
            text: text.to_string(),
            base,
        });
        base
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// The index of the file containing a global offset. An offset just
    /// past the end of a file (an end-of-file span) belongs to that file.
    pub fn file_index(&self, offset: usize) -> Option<usize> {
        self.files
            .iter()
            .position(|f| offset >= f.base && offset <= f.end())
    }

    pub fn file(&self, offset: usize) -> Option<&SourceFile> {
        self.file_index(offset).map(|i| &self.files[i])
    }

    /// The file and line/column of a global offset.
    pub fn location(&self, offset: usize) -> Option<(&SourceFile, LineCol)> {
        let file = self.file(offset)?;
        let local = (offset - file.base).min(file.text.len());
        let before = &file.text[..floor_char_boundary(&file.text, local)];
        let line = before.matches('\n').count() as u32 + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let col = before[line_start..].chars().count() as u32 + 1;
        Some((file, LineCol { line, col }))
    }
}

fn floor_char_boundary(text: &str, mut i: usize) -> usize {
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_get_separate_offsets() {
        let mut map = SourceMap::new();
        assert_eq!(map.add("a.tds", "abc\ndef"), 0);
        assert_eq!(map.add("b.tds", "xyz"), 8);
        assert_eq!(map.file(2).unwrap().name, "a.tds");
        assert_eq!(map.file(7).unwrap().name, "a.tds"); // end of a.tds
        assert_eq!(map.file(9).unwrap().name, "b.tds");
        let (file, lc) = map.location(5).unwrap();
        assert_eq!(
            (file.name.as_str(), lc),
            ("a.tds", LineCol { line: 2, col: 2 })
        );
        let (file, lc) = map.location(10).unwrap();
        assert_eq!(
            (file.name.as_str(), lc),
            ("b.tds", LineCol { line: 1, col: 3 })
        );
    }
}
