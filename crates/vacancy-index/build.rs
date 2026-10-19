//! The elevated reader's Windows dressing: a version resource, so Windows' admin prompt names it "Vacancy scan
//! helper", a manifest, and loading its DLLs from System32 only (its folder may be writable by the user).

fn main() {
    println!("cargo:rerun-if-changed=reader.rc");
    println!("cargo:rerun-if-changed=reader.manifest");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !windows {
        return;
    }
    // The reader's DLLs must load from System32 only, which takes MSVC's linker: never build it without.
    assert!(
        msvc,
        "vacancy-index-reader is built with MSVC only: its DLLs must load from System32 (/DEPENDENTLOADFLAG)"
    );
    // LOAD_LIBRARY_SEARCH_SYSTEM32 for the DLLs the reader imports: none come from its own folder.
    println!("cargo:rustc-link-arg-bin=vacancy-index-reader=/DEPENDENTLOADFLAG:0x800");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut parts = version
        .split(['.', '-'])
        .map(|p| p.parse::<u16>().unwrap_or(0));
    let mut next = || parts.next().unwrap_or(0);
    let numbers = format!("{},{},{},0", next(), next(), next());
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let manifest = std::path::Path::new(&dir).join("reader.manifest");
    let rc = std::fs::read_to_string(std::path::Path::new(&dir).join("reader.rc"))
        .expect("reader.rc")
        .replace("VACANCY_VERSION_NUM", &numbers)
        .replace("VACANCY_VERSION_STR", &format!("\"{version}\""))
        .replace(
            "VACANCY_MANIFEST",
            &format!(
                "\"{}\"",
                manifest.display().to_string().replace('\\', "\\\\")
            ),
        );
    let out = std::path::Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"))
        .join("reader.rc");
    std::fs::write(&out, rc).expect("write the reader's resource script");
    embed_resource::compile_for(&out, ["vacancy-index-reader"], embed_resource::NONE)
        .manifest_required()
        .expect("the reader's resources compile");
}
