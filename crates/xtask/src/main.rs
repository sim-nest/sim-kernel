#![forbid(unsafe_code)]

mod file_sizes;
mod simdoc;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().collect::<Vec<_>>();
    let program = args.first().map(String::as_str).unwrap_or("xtask");
    match args.get(1).map(String::as_str) {
        Some("simdoc") => simdoc::run(args),
        Some("check-file-sizes") => {
            if args.len() != 2 {
                return Err(format!("usage: {program} check-file-sizes"));
            }
            let root = std::env::current_dir().map_err(|err| format!("current dir: {err}"))?;
            file_sizes::run(&root)
        }
        _ => Err(format!("usage: {program} <simdoc|check-file-sizes> ...")),
    }
}
