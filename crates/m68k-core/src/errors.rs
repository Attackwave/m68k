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
///
/// `filename` empty means the location is not resolved yet: `line_no`
/// then counts lines of the preprocessed text and the assembler maps it
/// to a file and line through its source map. A diagnostic that names its
/// file is final.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub line_no: Option<usize>,
    pub filename: String,
    /// Macro the line was expanded from, once the location is resolved.
    pub macro_name: Option<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, line_no: Option<usize>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            line_no,
            filename: String::new(),
            macro_name: None,
        }
    }

    pub fn warning(message: impl Into<String>, line_no: Option<usize>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            line_no,
            filename: String::new(),
            macro_name: None,
        }
    }
}

impl From<AsmError> for Diagnostic {
    fn from(e: AsmError) -> Self {
        Self {
            severity: Severity::Error,
            message: e.message,
            line_no: e.line_no,
            filename: e.file.unwrap_or_default(),
            macro_name: None,
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

        let context = match &self.macro_name {
            Some(m) => format!(" (in macro {m})"),
            None => String::new(),
        };
        if loc.is_empty() {
            write!(f, "{}: {}", prefix, self.message)
        } else {
            write!(f, "{}{}: {}: {}", loc, context, prefix, self.message)
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

    pub fn push(&mut self, diagnostic: Diagnostic) {
        match diagnostic.severity {
            Severity::Error => self.errors.push(diagnostic),
            Severity::Warning => self.warnings.push(diagnostic),
        }
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Drop collected diagnostics, keeping the file name.
    pub fn clear(&mut self) {
        self.errors.clear();
        self.warnings.clear();
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
