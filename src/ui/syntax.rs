//! A small syntax highlighter for the diff and code views.
//!
//! Each language is a table (keywords, comments, strings...) read by one
//! lexer, line by line. No grammar files are loaded: it costs no memory at
//! startup and is fast enough to run on every redraw. Multi-line comments
//! and strings are followed from one line to the next through [`State`].

/// What a piece of a line is, to pick its color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Keyword,
    /// Built-in types and well-known library types (`std_logic`, `u32`...).
    Type,
    Comment,
    String,
    Number,
    /// Preprocessor lines, Verilog directives and system tasks, VHDL
    /// attributes, decorators.
    Attribute,
}

/// Where the previous line left the lexer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Normal,
    /// Inside a block comment that has not been closed yet.
    BlockComment,
    /// Inside a string that runs over several lines, closed by this text.
    LongString(&'static str),
}

pub struct Language {
    /// Shown next to the diff legend.
    pub name: &'static str,
    extensions: &'static [&'static str],
    /// Exact file names (`Makefile`, `CMakeLists.txt`...).
    file_names: &'static [&'static str],
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    /// Quote characters of one-line strings.
    quotes: &'static [char],
    /// Delimiters of strings that may span several lines (`"""`).
    long_strings: &'static [&'static str],
    /// Sorted, lowercase when `ignore_case`.
    keywords: &'static [&'static str],
    /// Sorted, lowercase when `ignore_case`.
    types: &'static [&'static str],
    ignore_case: bool,
    /// Characters that start an attribute-like word (`$display`, `@decorator`).
    attribute_prefixes: &'static [char],
    /// A `#` at the start of a line begins a preprocessor directive.
    preprocessor: bool,
    /// VHDL: `'` is a character literal (`'1'`) or an attribute (`clk'event`).
    vhdl_tick: bool,
    /// Verilog: `'` starts a sized number (`8'hFF`).
    verilog_tick: bool,
    /// `$var`, `${var}` in shells and makefiles.
    dollar_variables: bool,
}

const fn lang(name: &'static str) -> Language {
    Language {
        name,
        extensions: &[],
        file_names: &[],
        line_comments: &[],
        block_comment: None,
        quotes: &['"'],
        long_strings: &[],
        keywords: &[],
        types: &[],
        ignore_case: false,
        attribute_prefixes: &[],
        preprocessor: false,
        vhdl_tick: false,
        verilog_tick: false,
        dollar_variables: false,
    }
}

const C_BLOCK: Option<(&str, &str)> = Some(("/*", "*/"));

static VHDL: Language = Language {
    extensions: &["vhd", "vhdl", "vho"],
    line_comments: &["--"],
    block_comment: C_BLOCK,
    keywords: &[
        "abs",
        "access",
        "after",
        "alias",
        "all",
        "and",
        "architecture",
        "array",
        "assert",
        "attribute",
        "begin",
        "block",
        "body",
        "buffer",
        "bus",
        "case",
        "component",
        "configuration",
        "constant",
        "context",
        "disconnect",
        "downto",
        "else",
        "elsif",
        "end",
        "entity",
        "exit",
        "file",
        "for",
        "force",
        "function",
        "generate",
        "generic",
        "group",
        "guarded",
        "if",
        "impure",
        "in",
        "inertial",
        "inout",
        "is",
        "label",
        "library",
        "linkage",
        "literal",
        "loop",
        "map",
        "mod",
        "nand",
        "new",
        "next",
        "nor",
        "not",
        "null",
        "of",
        "on",
        "open",
        "or",
        "others",
        "out",
        "package",
        "parameter",
        "port",
        "postponed",
        "procedure",
        "process",
        "protected",
        "pure",
        "range",
        "record",
        "register",
        "reject",
        "release",
        "rem",
        "report",
        "return",
        "rol",
        "ror",
        "select",
        "severity",
        "shared",
        "signal",
        "sla",
        "sll",
        "sra",
        "srl",
        "subtype",
        "then",
        "to",
        "transport",
        "type",
        "unaffected",
        "units",
        "until",
        "use",
        "variable",
        "wait",
        "when",
        "while",
        "with",
        "xnor",
        "xor",
    ],
    types: &[
        "bit",
        "bit_vector",
        "boolean",
        "boolean_vector",
        "character",
        "integer",
        "integer_vector",
        "line",
        "natural",
        "positive",
        "real",
        "real_vector",
        "severity_level",
        "signed",
        "std_logic",
        "std_logic_vector",
        "std_ulogic",
        "std_ulogic_vector",
        "string",
        "text",
        "time",
        "time_vector",
        "unsigned",
    ],
    ignore_case: true,
    vhdl_tick: true,
    ..lang("VHDL")
};

static VERILOG: Language = Language {
    extensions: &["v", "vh", "sv", "svh", "svi", "vams"],
    line_comments: &["//"],
    block_comment: C_BLOCK,
    keywords: &[
        "always",
        "always_comb",
        "always_ff",
        "always_latch",
        "and",
        "assert",
        "assign",
        "assume",
        "automatic",
        "begin",
        "bind",
        "break",
        "buf",
        "case",
        "casex",
        "casez",
        "class",
        "clocking",
        "const",
        "constraint",
        "continue",
        "cover",
        "covergroup",
        "deassign",
        "default",
        "defparam",
        "disable",
        "do",
        "else",
        "end",
        "endcase",
        "endclass",
        "endclocking",
        "endfunction",
        "endgenerate",
        "endgroup",
        "endinterface",
        "endmodule",
        "endpackage",
        "endprogram",
        "endproperty",
        "endsequence",
        "endtask",
        "enum",
        "event",
        "export",
        "extends",
        "extern",
        "final",
        "for",
        "force",
        "foreach",
        "forever",
        "fork",
        "function",
        "generate",
        "genvar",
        "if",
        "iff",
        "import",
        "initial",
        "inout",
        "input",
        "inside",
        "interface",
        "join",
        "join_any",
        "join_none",
        "localparam",
        "modport",
        "module",
        "nand",
        "negedge",
        "new",
        "nor",
        "not",
        "null",
        "or",
        "output",
        "package",
        "packed",
        "parameter",
        "posedge",
        "priority",
        "program",
        "property",
        "pure",
        "rand",
        "randc",
        "release",
        "repeat",
        "return",
        "sequence",
        "signed",
        "static",
        "struct",
        "super",
        "task",
        "this",
        "typedef",
        "union",
        "unique",
        "unsigned",
        "virtual",
        "void",
        "wait",
        "while",
        "xnor",
        "xor",
    ],
    types: &[
        "bit",
        "byte",
        "chandle",
        "int",
        "integer",
        "logic",
        "longint",
        "real",
        "realtime",
        "reg",
        "shortint",
        "shortreal",
        "string",
        "supply0",
        "supply1",
        "time",
        "tri",
        "tri0",
        "tri1",
        "uwire",
        "wand",
        "wire",
        "wor",
    ],
    attribute_prefixes: &['$', '`'],
    verilog_tick: true,
    ..lang("Verilog")
};

static C_FAMILY: Language = Language {
    extensions: &[
        "c", "h", "cc", "cpp", "cxx", "c++", "hh", "hpp", "hxx", "h++", "ino", "cu", "m", "mm",
    ],
    line_comments: &["//"],
    block_comment: C_BLOCK,
    quotes: &['"', '\''],
    keywords: &[
        "alignas",
        "alignof",
        "auto",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "consteval",
        "constexpr",
        "constinit",
        "continue",
        "decltype",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "explicit",
        "export",
        "extern",
        "false",
        "final",
        "for",
        "friend",
        "goto",
        "if",
        "inline",
        "mutable",
        "namespace",
        "new",
        "noexcept",
        "nullptr",
        "operator",
        "override",
        "private",
        "protected",
        "public",
        "register",
        "restrict",
        "return",
        "sizeof",
        "static",
        "static_assert",
        "struct",
        "switch",
        "template",
        "this",
        "throw",
        "true",
        "try",
        "typedef",
        "typename",
        "union",
        "using",
        "virtual",
        "volatile",
        "while",
    ],
    types: &[
        "bool",
        "char",
        "double",
        "float",
        "int",
        "int16_t",
        "int32_t",
        "int64_t",
        "int8_t",
        "long",
        "ptrdiff_t",
        "short",
        "signed",
        "size_t",
        "ssize_t",
        "std",
        "string",
        "uint16_t",
        "uint32_t",
        "uint64_t",
        "uint8_t",
        "uintptr_t",
        "unsigned",
        "vector",
        "void",
    ],
    preprocessor: true,
    ..lang("C/C++")
};

static RUST: Language = Language {
    extensions: &["rs"],
    line_comments: &["//"],
    block_comment: C_BLOCK,
    quotes: &['"'],
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true",
        "type", "unsafe", "use", "where", "while",
    ],
    types: &[
        "Box", "Option", "Result", "Self", "String", "Vec", "bool", "char", "f32", "f64", "i128",
        "i16", "i32", "i64", "i8", "isize", "str", "u128", "u16", "u32", "u64", "u8", "usize",
    ],
    attribute_prefixes: &['#'],
    ..lang("Rust")
};

static PYTHON: Language = Language {
    extensions: &["py", "pyw", "pyi"],
    file_names: &["SConstruct", "SConscript"],
    line_comments: &["#"],
    quotes: &['"', '\''],
    long_strings: &["\"\"\"", "'''"],
    keywords: &[
        "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
        "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global",
        "if", "import", "in", "is", "lambda", "match", "nonlocal", "not", "or", "pass", "raise",
        "return", "self", "try", "while", "with", "yield",
    ],
    types: &[
        "bool",
        "bytes",
        "dict",
        "float",
        "frozenset",
        "int",
        "list",
        "object",
        "set",
        "str",
        "tuple",
    ],
    attribute_prefixes: &['@'],
    ..lang("Python")
};

static SHELL: Language = Language {
    extensions: &["sh", "bash", "zsh", "ksh", "bashrc", "profile", "env"],
    line_comments: &["#"],
    quotes: &['"', '\''],
    keywords: &[
        "case", "do", "done", "echo", "elif", "else", "esac", "exit", "export", "fi", "for",
        "function", "if", "in", "local", "readonly", "return", "set", "shift", "source", "then",
        "unset", "until", "while",
    ],
    dollar_variables: true,
    ..lang("Shell")
};

/// Tcl, also used by FPGA tools for constraints (`.xdc`, `.sdc`) and scripts.
static TCL: Language = Language {
    extensions: &["tcl", "xdc", "sdc", "do", "qsf", "tk"],
    line_comments: &["#"],
    keywords: &[
        "after",
        "append",
        "break",
        "catch",
        "continue",
        "else",
        "elseif",
        "error",
        "eval",
        "expr",
        "for",
        "foreach",
        "global",
        "if",
        "incr",
        "lappend",
        "lindex",
        "list",
        "namespace",
        "package",
        "proc",
        "puts",
        "return",
        "set",
        "source",
        "switch",
        "unset",
        "upvar",
        "variable",
        "while",
    ],
    types: &[
        "add_files",
        "create_clock",
        "create_generated_clock",
        "create_project",
        "current_design",
        "get_cells",
        "get_clocks",
        "get_nets",
        "get_pins",
        "get_ports",
        "launch_runs",
        "read_verilog",
        "read_vhdl",
        "read_xdc",
        "set_false_path",
        "set_input_delay",
        "set_location_assignment",
        "set_max_delay",
        "set_multicycle_path",
        "set_output_delay",
        "set_property",
        "synth_design",
    ],
    dollar_variables: true,
    ..lang("Tcl")
};

static JAVASCRIPT: Language = Language {
    extensions: &["js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts"],
    line_comments: &["//"],
    block_comment: C_BLOCK,
    quotes: &['"', '\''],
    long_strings: &["`"],
    keywords: &[
        "as",
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "from",
        "function",
        "if",
        "implements",
        "import",
        "in",
        "instanceof",
        "interface",
        "let",
        "new",
        "null",
        "of",
        "private",
        "protected",
        "public",
        "readonly",
        "return",
        "static",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "type",
        "typeof",
        "undefined",
        "var",
        "void",
        "while",
        "yield",
    ],
    types: &[
        "Array", "Map", "Promise", "Set", "any", "boolean", "never", "number", "object", "string",
        "unknown",
    ],
    attribute_prefixes: &['@'],
    ..lang("JavaScript")
};

/// Java, C#, Kotlin, Go, Swift, Scala: C-like syntax with a shared keyword list.
static C_LIKE: Language = Language {
    extensions: &[
        "java", "cs", "kt", "kts", "go", "swift", "scala", "dart", "groovy", "gradle",
    ],
    line_comments: &["//"],
    block_comment: C_BLOCK,
    quotes: &['"', '\''],
    long_strings: &["\"\"\"", "`"],
    keywords: &[
        "abstract",
        "break",
        "case",
        "catch",
        "chan",
        "class",
        "const",
        "continue",
        "def",
        "default",
        "defer",
        "do",
        "else",
        "enum",
        "extends",
        "false",
        "final",
        "finally",
        "for",
        "fun",
        "func",
        "go",
        "if",
        "implements",
        "import",
        "in",
        "interface",
        "internal",
        "is",
        "let",
        "map",
        "namespace",
        "new",
        "nil",
        "null",
        "object",
        "override",
        "package",
        "private",
        "protected",
        "public",
        "range",
        "return",
        "select",
        "static",
        "struct",
        "super",
        "switch",
        "this",
        "throw",
        "throws",
        "true",
        "try",
        "type",
        "using",
        "val",
        "var",
        "void",
        "when",
        "while",
    ],
    types: &[
        "Boolean", "Int", "Integer", "List", "Long", "String", "bool", "boolean", "byte", "char",
        "double", "error", "float", "float64", "int", "int64", "long", "rune", "short", "string",
        "uint", "uint64",
    ],
    attribute_prefixes: &['@'],
    ..lang("Java/C#/Go")
};

static CONFIG: Language = Language {
    extensions: &[
        "toml",
        "ini",
        "cfg",
        "conf",
        "properties",
        "yaml",
        "yml",
        "gitignore",
        "gitattributes",
        "gitmodules",
        "editorconfig",
    ],
    file_names: &[
        ".gitignore",
        ".gitattributes",
        ".gitmodules",
        ".editorconfig",
    ],
    line_comments: &["#", ";"],
    quotes: &['"', '\''],
    keywords: &["false", "no", "null", "off", "on", "true", "yes"],
    ignore_case: true,
    ..lang("INI/TOML/YAML")
};

static JSON: Language = Language {
    extensions: &["json", "jsonc", "json5"],
    line_comments: &["//"],
    block_comment: C_BLOCK,
    keywords: &["false", "null", "true"],
    ..lang("JSON")
};

static MAKE: Language = Language {
    extensions: &["mk", "mak", "cmake"],
    file_names: &["Makefile", "makefile", "GNUmakefile", "CMakeLists.txt"],
    line_comments: &["#"],
    quotes: &['"', '\''],
    keywords: &[
        "define", "else", "endef", "endif", "export", "ifdef", "ifeq", "ifndef", "ifneq",
        "include", "override",
    ],
    dollar_variables: true,
    ..lang("Make")
};

static LANGUAGES: [&Language; 12] = [
    &VHDL,
    &VERILOG,
    &C_FAMILY,
    &RUST,
    &PYTHON,
    &SHELL,
    &TCL,
    &JAVASCRIPT,
    &C_LIKE,
    &CONFIG,
    &JSON,
    &MAKE,
];

/// The language of a file, from its name, or `None` to show it plain.
pub fn language_for(path: &str) -> Option<&'static Language> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if let Some(lang) = LANGUAGES.iter().find(|l| l.file_names.contains(&name)) {
        return Some(lang);
    }
    let (_, ext) = name.rsplit_once('.')?;
    LANGUAGES
        .iter()
        .find(|l| l.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)))
        .copied()
}

/// Splits `line` into colored pieces, as byte ranges in `line`. Adjacent
/// pieces of the same kind are merged; the pieces cover the whole line.
pub fn highlight(lang: &Language, line: &str, state: &mut State) -> Vec<(usize, usize, Kind)> {
    let mut out = Pieces(Vec::new());
    let bytes = line.as_bytes();
    let mut i = 0;

    match *state {
        State::Normal => {}
        State::BlockComment => {
            let (_, close) = lang.block_comment.expect("block comment state");
            i = close_at(line, 0, close);
            out.push(0, i, Kind::Comment);
            if i <= line.len() && line[..i].ends_with(close) {
                *state = State::Normal;
            }
        }
        State::LongString(close) => {
            i = close_at(line, 0, close);
            out.push(0, i, Kind::String);
            if line[..i].ends_with(close) {
                *state = State::Normal;
            }
        }
    }
    if *state != State::Normal {
        return out.0;
    }

    let first_word = line.len() - line.trim_start().len();
    while i < line.len() {
        let rest = &line[i..];
        let c = rest.chars().next().unwrap_or(' ');

        if lang.line_comments.iter().any(|m| rest.starts_with(m)) {
            // `#` in shells only starts a comment at a word boundary (`$#`).
            let boundary = i == 0 || bytes[i - 1].is_ascii_whitespace() || c != '#';
            if boundary && !(lang.preprocessor && c == '#') {
                out.push(i, line.len(), Kind::Comment);
                break;
            }
        }
        if let Some((open, close)) = lang.block_comment
            && rest.starts_with(open)
        {
            let end = close_at(line, i + open.len(), close);
            out.push(i, end, Kind::Comment);
            if !line[i + open.len()..end].ends_with(close) {
                *state = State::BlockComment;
            }
            i = end;
            continue;
        }
        if lang.preprocessor && c == '#' && i == first_word {
            let end = word_end(line, i + 1);
            out.push(i, end, Kind::Attribute);
            i = end;
            continue;
        }
        if let Some(delim) = lang.long_strings.iter().find(|d| rest.starts_with(**d)) {
            let end = close_at(line, i + delim.len(), delim);
            out.push(i, end, Kind::String);
            if !line[i + delim.len()..end].ends_with(*delim) {
                *state = State::LongString(delim);
            }
            i = end;
            continue;
        }
        if lang.quotes.contains(&c) {
            let end = string_end(line, i + 1, c);
            out.push(i, end, Kind::String);
            i = end;
            continue;
        }
        if c == '\'' && lang.vhdl_tick {
            // '1' is a character; clk'event is an attribute.
            if rest.len() >= 3 && rest.as_bytes()[2] == b'\'' {
                out.push(i, i + 3, Kind::String);
                i += 3;
            } else {
                let end = word_end(line, i + 1);
                out.push(i, end, Kind::Attribute);
                i = end;
            }
            continue;
        }
        if c == '\'' && lang.verilog_tick {
            // 8'hFF, 'b0, '0
            let mut end = i + 1;
            if matches!(bytes.get(end), Some(b's' | b'S')) {
                end += 1;
            }
            end = word_end(line, end);
            out.push(i, end.max(i + 1), Kind::Number);
            i = end.max(i + 1);
            continue;
        }
        if lang.dollar_variables && c == '$' {
            let end = match bytes.get(i + 1) {
                Some(b'{') | Some(b'(') => {
                    let close = if bytes[i + 1] == b'{' { '}' } else { ')' };
                    line[i..].find(close).map_or(line.len(), |p| i + p + 1)
                }
                _ => word_end(line, i + 1).max(i + 2).min(line.len()),
            };
            out.push(i, end, Kind::Attribute);
            i = end;
            continue;
        }
        if lang.attribute_prefixes.contains(&c) {
            let mut end = word_end(line, i + 1);
            if c == '#' && bytes.get(i + 1) == Some(&b'[') {
                // Rust attribute: #[derive(...)]
                end = line[i..].find(']').map_or(line.len(), |p| i + p + 1);
            } else if c == '#' {
                out.push(i, i + 1, Kind::Plain);
                i += 1;
                continue;
            }
            out.push(i, end.max(i + 1), Kind::Attribute);
            i = end.max(i + 1);
            continue;
        }
        if c.is_ascii_digit() && !preceded_by_word(bytes, i) {
            let end = number_end(line, i);
            out.push(i, end, Kind::Number);
            i = end;
            continue;
        }
        if is_word_char(c) {
            let end = word_end(line, i);
            let word = &line[i..end];
            out.push(i, end, word_kind(lang, word));
            i = end;
            continue;
        }
        out.push(i, i + c.len_utf8(), Kind::Plain);
        i += c.len_utf8();
    }
    out.0
}

struct Pieces(Vec<(usize, usize, Kind)>);

impl Pieces {
    fn push(&mut self, start: usize, end: usize, kind: Kind) {
        if start >= end {
            return;
        }
        match self.0.last_mut() {
            Some(last) if last.2 == kind && last.1 == start => last.1 = end,
            _ => self.0.push((start, end, kind)),
        }
    }
}

fn word_kind(lang: &Language, word: &str) -> Kind {
    let found = |list: &[&str]| {
        if lang.ignore_case && word.bytes().any(|b| b.is_ascii_uppercase()) {
            list.binary_search(&word.to_ascii_lowercase().as_str())
                .is_ok()
        } else {
            list.binary_search(&word).is_ok()
        }
    };
    if found(lang.keywords) {
        Kind::Keyword
    } else if found(lang.types) {
        Kind::Type
    } else {
        Kind::Plain
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn preceded_by_word(bytes: &[u8], i: usize) -> bool {
    i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')
}

fn word_end(line: &str, from: usize) -> usize {
    line[from..]
        .char_indices()
        .find(|(_, c)| !is_word_char(*c))
        .map_or(line.len(), |(p, _)| from + p)
}

/// Digits, hex letters, `_`, `.`, exponents and suffixes: `0x1F`, `3.0e-9`,
/// `10_000`, `16#FF#` (VHDL), `8'hFF` (Verilog), `42u32`.
fn number_end(line: &str, from: usize) -> usize {
    let bytes = line.as_bytes();
    let mut end = from;
    while end < bytes.len() {
        let b = bytes[end];
        let exponent_sign = matches!(b, b'+' | b'-') && matches!(bytes[end - 1], b'e' | b'E');
        let next = bytes.get(end + 1).copied().unwrap_or(b' ');
        // `.5` in 3.5; Verilog sized literal: 8'hFF
        let joined =
            (b == b'.' && next.is_ascii_digit()) || (b == b'\'' && next.is_ascii_alphabetic());
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'#' || exponent_sign || joined {
            end += 1;
        } else {
            break;
        }
    }
    end
}

/// End of a one-line string opened just before `from`, after its closing
/// quote (or the end of the line when it is not closed).
fn string_end(line: &str, from: usize, quote: char) -> usize {
    let mut chars = line[from..].char_indices();
    while let Some((p, c)) = chars.next() {
        if c == '\\' {
            chars.next();
        } else if c == quote {
            return from + p + c.len_utf8();
        }
    }
    line.len()
}

/// Position just after `close` from `from`, or the end of the line.
fn close_at(line: &str, from: usize, close: &str) -> usize {
    line[from.min(line.len())..]
        .find(close)
        .map_or(line.len(), |p| from + p + close.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds<'a>(lang: &Language, line: &'a str) -> Vec<(&'a str, Kind)> {
        let mut state = State::Normal;
        highlight(lang, line, &mut state)
            .into_iter()
            .filter(|p| p.2 != Kind::Plain)
            .map(|(s, e, k)| (&line[s..e], k))
            .collect()
    }

    #[test]
    fn word_lists_are_sorted_for_binary_search() {
        for lang in LANGUAGES {
            for list in [lang.keywords, lang.types] {
                let mut sorted = list.to_vec();
                sorted.sort_unstable();
                assert_eq!(list, sorted.as_slice(), "{} list not sorted", lang.name);
                if lang.ignore_case {
                    assert!(list.iter().all(|w| *w == w.to_lowercase()), "{}", lang.name);
                }
            }
        }
    }

    #[test]
    fn languages_from_file_names() {
        assert_eq!(language_for("rtl/top.vhd").unwrap().name, "VHDL");
        assert_eq!(language_for("rtl\\TOP.VHDL").unwrap().name, "VHDL");
        assert_eq!(language_for("tb/uart.sv").unwrap().name, "Verilog");
        assert_eq!(language_for("constraints/pins.xdc").unwrap().name, "Tcl");
        assert_eq!(language_for("sw/Makefile").unwrap().name, "Make");
        assert_eq!(language_for(".gitignore").unwrap().name, "INI/TOML/YAML");
        assert!(language_for("notes.txt").is_none());
        assert!(language_for("README").is_none());
    }

    #[test]
    fn pieces_cover_the_whole_line() {
        let line = "  signal count : unsigned(7 downto 0) := (others => '0'); -- counter";
        let mut state = State::Normal;
        let pieces = highlight(&VHDL, line, &mut state);
        assert_eq!(pieces.first().unwrap().0, 0);
        assert_eq!(pieces.last().unwrap().1, line.len());
        for pair in pieces.windows(2) {
            assert_eq!(pair[0].1, pair[1].0);
        }
    }

    #[test]
    fn vhdl() {
        let got = kinds(
            &VHDL,
            "  if rising_edge(clk) and clk'event then q <= x\"FF\"; -- note",
        );
        assert_eq!(
            got,
            vec![
                ("if", Kind::Keyword),
                ("and", Kind::Keyword),
                ("'event", Kind::Attribute),
                ("then", Kind::Keyword),
                ("\"FF\"", Kind::String),
                ("-- note", Kind::Comment),
            ]
        );
        assert_eq!(
            kinds(&VHDL, "SIGNAL S : STD_LOGIC := '1';"),
            vec![
                ("SIGNAL", Kind::Keyword),
                ("STD_LOGIC", Kind::Type),
                ("'1'", Kind::String),
            ]
        );
        assert_eq!(kinds(&VHDL, "x := 16#FF#;"), vec![("16#FF#", Kind::Number)]);
    }

    #[test]
    fn verilog() {
        assert_eq!(
            kinds(&VERILOG, "always @(posedge clk) q <= 8'hFF; // reset"),
            vec![
                ("always", Kind::Keyword),
                ("posedge", Kind::Keyword),
                ("8'hFF", Kind::Number),
                ("// reset", Kind::Comment),
            ]
        );
        assert_eq!(
            kinds(&VERILOG, "`define W 8\n"),
            vec![("`define", Kind::Attribute), ("8", Kind::Number)]
        );
        assert_eq!(
            kinds(&VERILOG, "$display(\"%d\", 'b1);"),
            vec![
                ("$display", Kind::Attribute),
                ("\"%d\"", Kind::String),
                ("'b1", Kind::Number),
            ]
        );
    }

    #[test]
    fn block_comments_span_lines() {
        let mut state = State::Normal;
        let first = highlight(&C_FAMILY, "int a; /* start", &mut state);
        assert_eq!(first.last().unwrap().2, Kind::Comment);
        assert_eq!(state, State::BlockComment);
        let middle = highlight(&C_FAMILY, "still inside", &mut state);
        assert_eq!(middle, vec![(0, 12, Kind::Comment)]);
        let last = highlight(&C_FAMILY, "end */ return 1;", &mut state);
        assert_eq!(state, State::Normal);
        assert_eq!(last[0], (0, 6, Kind::Comment));
        assert!(last.iter().any(|p| p.2 == Kind::Keyword));
    }

    #[test]
    fn python_long_strings_and_decorators() {
        let mut state = State::Normal;
        highlight(&PYTHON, "doc = \"\"\"First line", &mut state);
        assert_eq!(state, State::LongString("\"\"\""));
        let end = highlight(&PYTHON, "end\"\"\" # done", &mut state);
        assert_eq!(state, State::Normal);
        assert_eq!(end.last().unwrap().2, Kind::Comment);
        assert_eq!(
            kinds(&PYTHON, "@cache\ndef f(): return None"),
            vec![
                ("@cache", Kind::Attribute),
                ("def", Kind::Keyword),
                ("return", Kind::Keyword),
                ("None", Kind::Keyword),
            ]
        );
    }

    #[test]
    fn c_preprocessor_and_strings() {
        assert_eq!(
            kinds(&C_FAMILY, "#include \"uart.h\""),
            vec![("#include", Kind::Attribute), ("\"uart.h\"", Kind::String)]
        );
        assert_eq!(
            kinds(&C_FAMILY, "char *s = \"a \\\" b\"; // x"),
            vec![
                ("char", Kind::Type),
                ("\"a \\\" b\"", Kind::String),
                ("// x", Kind::Comment),
            ]
        );
    }

    #[test]
    fn shell_hash_inside_words_is_not_a_comment() {
        assert_eq!(
            kinds(&SHELL, "echo $# ${HOME} # count"),
            vec![
                ("echo", Kind::Keyword),
                ("$#", Kind::Attribute),
                ("${HOME}", Kind::Attribute),
                ("# count", Kind::Comment),
            ]
        );
    }

    #[test]
    fn rust_attributes_and_numbers() {
        assert_eq!(
            kinds(&RUST, "#[derive(Debug)] let x: u32 = 0x1F + 3.5e-2;"),
            vec![
                ("#[derive(Debug)]", Kind::Attribute),
                ("let", Kind::Keyword),
                ("u32", Kind::Type),
                ("0x1F", Kind::Number),
                ("3.5e-2", Kind::Number),
            ]
        );
    }

    #[test]
    fn identifiers_with_digits_are_not_numbers() {
        assert_eq!(kinds(&VHDL, "reg_8 <= data2;"), vec![]);
    }
}
