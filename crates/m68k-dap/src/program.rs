//! The debugged executable: hunk layout, line table and symbols, placed at
//! the segment addresses the emulator reports.

use m68k_core::amiga_hunk::read_hunk_executable;

/// One line-table row at its runtime address.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub file: String,
    pub line: u32,
    pub address: u32,
}

#[derive(Debug, Default)]
pub struct Program {
    /// Runtime `(start, end)` of every hunk.
    pub hunks: Vec<(u32, u32)>,
    /// Sorted by address.
    pub lines: Vec<Line>,
    pub symbols: Vec<(String, u32)>,
}

impl Program {
    /// `segments` are the hunk start addresses from `qOffsets`, in hunk order.
    pub fn load(exe: &[u8], segments: &[u32]) -> Result<Self, String> {
        let parsed = read_hunk_executable(exe, 0).map_err(|e| e.0)?;
        if segments.len() < parsed.sections.len() {
            return Err(format!(
                "emulator reported {} segments for {} hunks",
                segments.len(),
                parsed.sections.len()
            ));
        }
        let mut program = Program::default();
        for (section, &seg) in parsed.sections.iter().zip(segments) {
            // The reader lays hunks out back to back from 0; LoadSeg does not.
            let at = |addr: u32| seg.wrapping_add(addr - section.address);
            program
                .hunks
                .push((seg, at(section.address + section.data.len() as u32)));
            program
                .lines
                .extend(section.lines.iter().map(|(file, line, addr)| Line {
                    file: file.clone(),
                    line: *line,
                    address: at(*addr),
                }));
            program.symbols.extend(
                section
                    .symbols
                    .iter()
                    .map(|(name, addr)| (name.clone(), at(*addr))),
            );
        }
        program.lines.sort_by_key(|l| l.address);
        Ok(program)
    }

    /// Whether `addr` lies inside one of the program's hunks.
    pub fn contains(&self, addr: u32) -> bool {
        self.hunks
            .iter()
            .any(|&(start, end)| (start..end).contains(&addr))
    }

    /// Source line containing `pc`: the last row at or below it in the same hunk.
    pub fn line_at(&self, pc: u32) -> Option<&Line> {
        let &(start, end) = self.hunks.iter().find(|(s, e)| (*s..*e).contains(&pc))?;
        let idx = self.lines.partition_point(|l| l.address <= pc);
        self.lines[..idx]
            .last()
            .filter(|l| l.address >= start && l.address < end)
    }

    /// Address for a breakpoint on `file:line`. A line without code moves to
    /// the next line in that file that has some; returns that line too.
    pub fn address_of(&self, file: &str, line: u32) -> Option<(u32, u32)> {
        self.lines
            .iter()
            .filter(|l| same_path(&l.file, file) && l.line >= line)
            .min_by_key(|l| (l.line, l.address))
            .map(|l| (l.address, l.line))
    }

    /// Nearest symbol at or below `addr`, as `name+offset`.
    pub fn symbolize(&self, addr: u32) -> Option<String> {
        let (name, at) = self
            .symbols
            .iter()
            .filter(|(_, a)| *a <= addr)
            .max_by_key(|(_, a)| *a)?;
        Some(match addr - at {
            0 => name.clone(),
            off => format!("{name}+{off}"),
        })
    }
}

/// Paths from the debug info and from the editor may differ in separators,
/// a Windows verbatim prefix (`\\?\`) and, on Windows, in case.
pub fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| {
        let p = p.strip_prefix(r"\\?\").unwrap_or(p).replace('\\', "/");
        if cfg!(windows) { p.to_lowercase() } else { p }
    };
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program() -> Program {
        let line = |line, address| Line {
            file: "/p/main.s".into(),
            line,
            address,
        };
        Program {
            hunks: vec![(0x1000, 0x1010)],
            lines: vec![line(3, 0x1000), line(5, 0x1004), line(9, 0x100a)],
            symbols: vec![("start".into(), 0x1000), ("loop".into(), 0x1004)],
        }
    }

    #[test]
    fn maps_pc_to_line_within_hunk() {
        let p = program();
        assert_eq!(p.line_at(0x1006).unwrap().line, 5);
        assert!(p.line_at(0x2000).is_none());
    }

    #[test]
    fn breakpoint_on_empty_line_moves_down() {
        let p = program();
        assert_eq!(p.address_of("/p/main.s", 4), Some((0x1004, 5)));
        assert_eq!(p.address_of("/p/main.s", 10), None);
    }

    #[test]
    fn verbatim_prefix_and_separators_do_not_matter() {
        assert!(same_path(r"\\?\C:/p/main.s", r"C:\p\main.s"));
        assert!(!same_path("/p/main.s", "/p/other.s"));
    }

    #[test]
    fn symbolizes_with_offset() {
        assert_eq!(program().symbolize(0x1006).as_deref(), Some("loop+2"));
    }
}
