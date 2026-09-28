//! Error types for the m68k assembler and disassembler.

use std::fmt;

/// Assembler error with optional line number.
#[derive(Debug, Clone)]
pub struct AsmError {
    pub message: String,
    pub line_no: Option<usize>,
    /// File the line number belongs to, when it is already known.
    ///
    /// Most errors are found after INCLUDE and macro expansion, so their
    /// line number refers to the expanded text and the caller translates
    /// it through the source map. An error raised *during* expansion has
    /// no map yet but does know its file, and its line already counts
    /// within that file — setting this marks the location as final so it
    /// is not translated a second time.
    pub file: Option<String>,
}

impl AsmError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            line_no: None,
            file: None,
        }
    }

    pub fn with_line(message: impl Into<String>, line_no: usize) -> Self {
        Self {
            message: message.into(),
            line_no: Some(line_no),
            file: None,
        }
    }

    /// An error whose location is already resolved: `file` and `line_no`
    /// name a real place in a real file and need no translation.
    pub fn at(message: impl Into<String>, file: impl Into<String>, line_no: usize) -> Self {
        Self {
            message: message.into(),
            line_no: Some(line_no),
            file: Some(file.into()),
        }
    }
}

impl std::error::Error for AsmError {}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line_no {
            Some(line) => write!(f, "line {}: {}", line, self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

/// Severity level for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A single diagnostic (error or warning).
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub line_no: Option<usize>,
    pub filename: String,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, line_no: Option<usize>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            line_no,
            filename: String::new(),
        }
    }

    pub fn warning(message: impl Into<String>, line_no: Option<usize>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            line_no,
            filename: String::new(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let loc = if !self.filename.is_empty() {
            match self.line_no {
                Some(line) => format!("{}:{}", self.filename, line),
                None => self.filename.clone(),
            }
        } else {
            match self.line_no {
                Some(line) => format!("line {}", line),
                None => String::new(),
            }
        };

        let prefix = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };

        if loc.is_empty() {
            write!(f, "{}: {}", prefix, self.message)
        } else {
            write!(f, "{}: {}: {}", loc, prefix, self.message)
        }
    }
}

/// Collects diagnostics during assembly.
#[derive(Debug, Default)]
pub struct ErrorCollector {
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
    pub filename: String,
}

impl ErrorCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn error(&mut self, message: impl Into<String>, line_no: Option<usize>) {
        self.errors.push(Diagnostic::error(message, line_no));
    }

    pub fn warning(&mut self, message: impl Into<String>, line_no: Option<usize>) {
        self.warnings.push(Diagnostic::warning(message, line_no));
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn format_errors(&self) -> String {
        let mut lines: Vec<String> = self.errors.iter().map(|d| d.to_string()).collect();
        lines.extend(self.warnings.iter().map(|d| d.to_string()));
        lines.join("\n")
    }
}

/// Exception containing multiple collected errors.
#[derive(Debug)]
pub struct MultiError {
    pub collector: ErrorCollector,
}

impl MultiError {
    pub fn new(collector: ErrorCollector) -> Self {
        Self { collector }
    }
}

impl std::error::Error for MultiError {}

impl fmt::Display for MultiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.collector.format_errors())
    }
}

/// Raised when an expression references an undefined symbol.
#[derive(Debug, Clone)]
pub struct UndefinedSymbol {
    pub name: String,
    pub line_no: usize,
}

impl UndefinedSymbol {
    pub fn new(name: impl Into<String>, line_no: usize) -> Self {
        Self {
            name: name.into(),
            line_no,
        }
    }
}

impl std::error::Error for UndefinedSymbol {}

impl fmt::Display for UndefinedSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: undefined symbol: {}", self.line_no, self.name)
    }
}
