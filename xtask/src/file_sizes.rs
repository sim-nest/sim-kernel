use std::fs;
use std::path::{Path, PathBuf};

const GENERAL_SOFT: usize = 500;
const GENERAL_HARD: usize = 700;
const ENTRY_SOFT: usize = 150;
const ENTRY_HARD: usize = 250;

pub fn run(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    collect_rs_files(&root.join("src"), &mut files)?;
    collect_rs_files(&root.join("xtask").join("src"), &mut files)?;
    files.sort();

    let mut soft_warnings = Vec::new();
    let mut hard_failures = Vec::new();
    for file in &files {
        let contents =
            fs::read_to_string(file).map_err(|err| format!("read {}: {err}", file.display()))?;
        let lines = contents.lines().count();
        let (soft, hard) = limits_for(file);
        if lines > hard {
            hard_failures.push(format!(
                "{} has {lines} line(s), hard limit {hard}",
                display_path(root, file)
            ));
        } else if lines > soft {
            soft_warnings.push(format!(
                "{} has {lines} line(s), soft target {soft}",
                display_path(root, file)
            ));
        }
    }

    for warning in &soft_warnings {
        println!("warning: {warning}");
    }
    if !hard_failures.is_empty() {
        return Err(format!(
            "check-file-sizes: {} hard failure(s)\n{}",
            hard_failures.len(),
            hard_failures.join("\n")
        ));
    }
    println!(
        "check-file-sizes: OK ({} Rust file(s), {} soft warning(s), 0 hard failure(s))",
        files.len(),
        soft_warnings.len()
    );
    Ok(())
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))? {
        let entry = entry.map_err(|err| format!("read {} entry: {err}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn limits_for(path: &Path) -> (usize, usize) {
    if path.file_name().is_some_and(|name| {
        matches!(
            name.to_str(),
            Some("lib.rs") | Some("main.rs") | Some("mod.rs")
        )
    }) {
        (ENTRY_SOFT, ENTRY_HARD)
    } else {
        (GENERAL_SOFT, GENERAL_HARD)
    }
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
