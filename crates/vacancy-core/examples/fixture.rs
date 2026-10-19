//! Writes a fixture profile for proving the executor, or lists a tree for a before-and-after comparison. Never point
//! it at a real profile: it refuses a folder that isn't empty, and one in or holding your home folder.
//!
//! ```text
//! cargo run -p vacancy-core --example fixture -- D:\tmp\vacancy-fixture-1
//! cargo run -p vacancy-core --example fixture -- --bulk 20000 D:\tmp\vacancy-fixture-2
//! cargo run -p vacancy-core --example fixture -- --advice D:\tmp\vacancy-advice-1
//! cargo run -p vacancy-core --example fixture -- --list D:\tmp\vacancy-fixture-1 before.tsv
//! ```
//!
//! `--bulk` adds that many small files to the profile's npm cache, so a run takes long enough to watch. Then start
//! the installed app with `VACANCY_FIXTURE_ROOT` set to the folder (see AGENTS.md, "Prove an app change").

use std::path::PathBuf;
use std::process::ExitCode;

use vacancy_core::fixture;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag, root, out] if flag == "--list" => list(root, out),
        [flag, root] if flag == "--advice" => write(root, 0, true),
        [flag, n, root] if flag == "--bulk" => match n.parse() {
            Ok(n) => write(root, n, false),
            Err(_) => {
                eprintln!("--bulk takes a number of files");
                ExitCode::FAILURE
            }
        },
        [root] => write(root, 0, false),
        _ => {
            eprintln!(
                "usage: fixture [--bulk <files> | --advice] <empty folder> | fixture --list <folder> <out.tsv>"
            );
            ExitCode::FAILURE
        }
    }
}

/// Writes the listing of `root` to `out`.
fn list(root: &str, out: &str) -> ExitCode {
    match fixture::inventory(&PathBuf::from(root)) {
        Ok(lines) => {
            if let Err(e) = std::fs::write(out, lines.join("\n") + "\n") {
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
    }
}

/// Writes a profile into `root`, with `bulk` more cache files.
fn write(root: &str, bulk: usize, advice: bool) -> ExitCode {
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
    let profile = if advice {
        fixture::build_with_advice(&root, bulk)
    } else {
        fixture::build_with(&root, bulk)
    };
    match profile {
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
            let d = &p.duplicates;
            println!(
                "  duplicates:        {} groups; the keeper rule keeps the first of each",
                d.groups.len()
            );
            for (keep, extras) in &d.groups {
                println!("    keep {}", keep.display());
                for e in extras {
                    println!("      extra {}", e.display());
                }
            }
            println!("  untick this group: {}", d.untick.display());
            println!(
                "  never proposed:    {} look-alikes",
                d.not_duplicates.len()
            );
            println!(
                "  tidy up (Windows): {} loose files in Downloads, Desktop and Documents",
                p.tidy.files.len()
            );
            println!("    collides:        {}", p.tidy.collision.display());
            println!(
                "    with:            {}",
                p.tidy.collision_existing.display()
            );
            println!("    also deleted:    {}", p.tidy.deleted_too.display());
            println!("    hold open:       {}", p.tidy.held_open.display());
            println!("    change after scan: {}", p.tidy.changed_later.display());
            if advice {
                println!(
                    "  advice: Steam hint (plus Logic instructions on macOS); fixture mode never opens Steam"
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("could not write the fixture: {e}");
            ExitCode::FAILURE
        }
    }
}
