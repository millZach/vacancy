//! Writes a fixture profile for proving the executor, or lists a tree for a before-and-after comparison. Never point
//! it at a real profile: it refuses a folder that isn't empty, and one in or holding your home folder.
//!
//! ```text
//! cargo run -p vacancy-core --example fixture -- D:\tmp\vacancy-fixture-1
//! cargo run -p vacancy-core --example fixture -- --list D:\tmp\vacancy-fixture-1 before.tsv
//! ```
//!
//! Then start the installed app with `VACANCY_FIXTURE_ROOT` set to the folder (see AGENTS.md, "Prove an app
//! change").

use std::path::PathBuf;
use std::process::ExitCode;

use vacancy_core::fixture;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag, root, out] if flag == "--list" => match fixture::inventory(&PathBuf::from(root)) {
            Ok(lines) => {
                let text = lines.join("\n") + "\n";
                if let Err(e) = std::fs::write(out, text) {
                    eprintln!("could not write {out}: {e}");
                    return ExitCode::FAILURE;
                }
                println!("{} entries listed in {out}", lines.len());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("could not list {root}: {e}");
                ExitCode::FAILURE
            }
        },
        [root] => {
            let root = PathBuf::from(root);
            let home = std::env::home_dir().unwrap_or_default();
            if std::fs::create_dir_all(&root).is_err() {
                eprintln!("could not make {}", root.display());
                return ExitCode::FAILURE;
            }
            if let Err(e) = fixture::check_root(&root, &home) {
                eprintln!("refused: {e}");
                return ExitCode::FAILURE;
            }
            match fixture::build(&root) {
                Ok(p) => {
                    println!("fixture profile written to {}", p.root.display());
                    println!("  home:              {}", p.home.display());
                    println!("  documents:         {}", p.documents.display());
                    println!("  link to documents: {}", p.link_to_documents.display());
                    println!("  hold open:         {}", p.held_open.display());
                    println!("  modify after scan: {}", p.modified_later.display());
                    println!("  read-only:         {}", p.read_only.display());
                    println!(
                        "  long path ({} chars): {}",
                        p.long_path.as_os_str().len(),
                        p.long_path.display()
                    );
                    println!("  node_modules:      {}", p.node_modules.display());
                    println!("  store file:        {}", p.store_file.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("could not write the fixture: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: fixture <empty folder> | fixture --list <folder> <out.tsv>");
            ExitCode::FAILURE
        }
    }
}
