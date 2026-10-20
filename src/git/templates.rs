//! Ready-made `.gitignore` sections for the tools the teams use, so build
//! outputs, simulator databases and lock files never end up in commits.

/// One template: a stable id (used for the labels), the name shown, and the
/// lines it adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub lines: &'static [&'static str],
}

pub const TEMPLATES: &[Template] = &[
    Template {
        id: "vivado",
        name: "Xilinx Vivado",
        lines: &[
            "*.jou",
            "*.log",
            "*.str",
            "*.cache/",
            "*.hw/",
            "*.runs/",
            "*.sim/",
            "*.ip_user_files/",
            "*.gen/",
            ".Xil/",
            "vivado_pid*.zip",
            "webtalk*",
        ],
    },
    Template {
        id: "quartus",
        name: "Intel Quartus",
        lines: &[
            "db/",
            "incremental_db/",
            "output_files/",
            "simulation/",
            "greybox_tmp/",
            "*.qws",
            "*.rpt",
            "*.summary",
            "*.smsg",
            "*.done",
            "*.sld",
            "c5_pin_model_dump.txt",
        ],
    },
    Template {
        id: "modelsim",
        name: "ModelSim / Questa",
        lines: &[
            "work/",
            "transcript",
            "*.wlf",
            "wlf*",
            "*.vstf",
            "modelsim.ini",
            "*.mti",
            "*.mpf",
        ],
    },
    Template {
        id: "ghdl",
        name: "GHDL / NVC",
        lines: &[
            "*.cf",
            "*.o",
            "*.ghw",
            "*.vcd",
            "*.fst",
            "e~*.o",
            "work-obj*.cf",
        ],
    },
    Template {
        id: "kicad",
        name: "KiCad",
        lines: &[
            "*-backups/",
            "*.kicad_prl",
            "fp-info-cache",
            "*.bak",
            "*.bck",
            "*.kicad_pcb-bak",
            "*.kicad_sch-bak",
            "~*.lck",
            "_autosave-*",
        ],
    },
    Template {
        id: "office",
        name: "Office",
        lines: &["~$*", ".~lock.*#"],
    },
    Template {
        id: "macos",
        name: "macOS",
        lines: &[
            ".DS_Store",
            "._*",
            ".AppleDouble",
            ".Spotlight-V100",
            ".Trashes",
        ],
    },
    Template {
        id: "windows",
        name: "Windows",
        lines: &["Thumbs.db", "ehthumbs.db", "Desktop.ini", "$RECYCLE.BIN/"],
    },
    Template {
        id: "linux",
        name: "Linux",
        lines: &["*~", ".directory", ".Trash-*", ".nfs*"],
    },
    Template {
        id: "vscode",
        name: "VS Code",
        lines: &[
            ".vscode/*",
            "!.vscode/settings.json",
            "!.vscode/extensions.json",
        ],
    },
    Template {
        id: "python",
        name: "Python",
        lines: &[
            "__pycache__/",
            "*.py[cod]",
            ".venv/",
            "venv/",
            ".pytest_cache/",
        ],
    },
    Template {
        id: "rust",
        name: "Rust",
        lines: &["target/", "**/*.rs.bk"],
    },
];

pub fn find(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.id == id)
}

/// The header line written before a template's rules.
pub fn header(template: &Template) -> String {
    format!("# {}", template.name)
}

/// Whether every rule of the template is already in the file.
pub fn applied(content: &str, template: &Template) -> bool {
    let present = lines(content);
    template.lines.iter().all(|l| present.contains(l))
}

fn lines(content: &str) -> Vec<&str> {
    content.lines().map(str::trim).collect()
}

/// `content` with the template's missing rules appended under a header.
/// `None` when nothing is missing.
pub fn append(content: &str, template: &Template) -> Option<String> {
    let present = lines(content);
    let missing: Vec<&str> = template
        .lines
        .iter()
        .copied()
        .filter(|l| !present.contains(l))
        .collect();
    if missing.is_empty() {
        return None;
    }
    let mut out = content.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&header(template));
    out.push('\n');
    for line in missing {
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        for (i, t) in TEMPLATES.iter().enumerate() {
            assert!(TEMPLATES[i + 1..].iter().all(|o| o.id != t.id), "{}", t.id);
            assert!(!t.lines.is_empty());
        }
    }

    #[test]
    fn appends_only_missing_rules() {
        let rust = find("rust").unwrap();
        assert_eq!(append("", rust).unwrap(), "# Rust\ntarget/\n**/*.rs.bk\n");
        assert_eq!(
            append("*.log\ntarget/", rust).unwrap(),
            "*.log\ntarget/\n\n# Rust\n**/*.rs.bk\n"
        );
        let done = append("", rust).unwrap();
        assert!(applied(&done, rust));
        assert_eq!(append(&done, rust), None);
        assert!(!applied("", rust));
    }
}
