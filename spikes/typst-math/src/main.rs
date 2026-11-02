//! `typst-math one <tex> [--display] [out.png]` renders one formula with the
//! app's renderer (`rike_native::math`).
//! `typst-math scan <dir>` renders every formula of every `.md` file under
//! `dir` (read only) and reports failures and timings.

use std::path::{Path, PathBuf};
use std::time::Instant;

use rike_native::math::{compile, to_typst};
use typst_math::formulas;

fn md_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            md_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("one") => {
            let tex = &args[1];
            let display = args.iter().any(|a| a == "--display");
            let out = args
                .iter()
                .skip(2)
                .find(|a| !a.starts_with("--"))
                .map_or("formula.png", String::as_str);
            println!(
                "typst: {}",
                to_typst(tex).unwrap_or_else(|e| format!("ERR {e}"))
            );
            let t = Instant::now();
            match compile(tex, display, false) {
                Ok(r) => {
                    std::fs::write(out, &r.png).unwrap();
                    println!(
                        "{}x{} px, {} bytes, {:?} -> {out}",
                        r.width,
                        r.height,
                        r.png.len(),
                        t.elapsed()
                    );
                }
                Err(e) => println!("FAILED: {e}"),
            }
        }
        Some("scan") => {
            let mut files = Vec::new();
            md_files(Path::new(&args[1]), &mut files);
            files.sort();
            let (mut ok, mut failed) = (0, 0);
            let mut times = Vec::new();
            let mut first = None;
            for file in &files {
                let Ok(text) = std::fs::read_to_string(file) else {
                    continue;
                };
                for (tex, display) in formulas(&text) {
                    let t = Instant::now();
                    match compile(&tex, display, false) {
                        Ok(_) => {
                            ok += 1;
                            let took = t.elapsed();
                            first.get_or_insert(took);
                            times.push(took);
                        }
                        Err(e) => {
                            failed += 1;
                            let short: String = tex.chars().take(70).collect();
                            println!(
                                "FAIL {} [{}]\n  tex: {short}\n  err: {e}",
                                file.display(),
                                if display { "display" } else { "inline" }
                            );
                        }
                    }
                }
            }
            times.sort();
            let pct = |p: usize| times.get(times.len().saturating_sub(1) * p / 100).copied();
            println!("formulas: {} ok, {} failed", ok, failed);
            println!(
                "render time: first {:?}, median {:?}, p95 {:?}, max {:?}",
                times.first(),
                pct(50),
                pct(95),
                times.last()
            );
        }
        _ => eprintln!("usage: typst-math one <tex> [--display] [out.png] | scan <dir>"),
    }
}
