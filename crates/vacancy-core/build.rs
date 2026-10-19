//! The app's build: the reader's SHA-256 (`VACANCY_READER_SHA256`, set by `tools/tauri.mjs` when it bundles the
//! reader) is compiled in with `option_env!`; a build with another reader must compile again, never keep the last
//! one's hash.

fn main() {
    println!("cargo:rerun-if-env-changed=VACANCY_READER_SHA256");
}
