//! Volume info against real folders on disk (a fresh temp dir), on whichever OS runs the tests.

use std::path::Path;

use vacancy_core::{VolumeError, list_volumes, machine_name, system_volume, volume_info};

#[test]
fn reads_the_volume_a_temp_dir_lives_on() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("probe.bin"), vec![7u8; 64 * 1024]).expect("write probe");

    let v = volume_info(dir.path()).expect("volume info for the temp dir");

    assert!(!v.name.trim().is_empty(), "the volume has a name: {v:?}");
    assert!(
        !v.mount_point.is_empty(),
        "the volume has a mount point: {v:?}"
    );
    assert!(v.total_bytes > 0, "the volume has a size: {v:?}");
    assert!(
        v.used_bytes > 0,
        "a volume holding a file is not empty: {v:?}"
    );
    assert!(v.used_bytes <= v.total_bytes, "used fits in total: {v:?}");

    // The mount point is itself on the same volume, and asking again from there gives the same answer.
    let again = volume_info(Path::new(&v.mount_point)).expect("volume info for the mount point");
    assert_eq!(again.mount_point, v.mount_point);
    assert_eq!(again.name, v.name);
    assert_eq!(again.total_bytes, v.total_bytes);

    // A folder inside the temp dir is on the same volume.
    let inner = dir.path().join("inner");
    std::fs::create_dir(&inner).expect("create inner");
    assert_eq!(
        volume_info(&inner)
            .expect("volume info for inner")
            .mount_point,
        v.mount_point
    );
}

#[cfg(windows)]
#[test]
fn a_windows_volume_is_a_drive_named_like_explorer() {
    let dir = tempfile::tempdir().expect("temp dir");
    let v = volume_info(dir.path()).expect("volume info");
    let path = dir.path().to_string_lossy().to_lowercase();
    assert!(
        path.starts_with(&v.mount_point.to_lowercase()),
        "{path} is under {}",
        v.mount_point
    );
    assert!(
        v.mount_point.ends_with('\\'),
        "a volume root ends with a backslash: {}",
        v.mount_point
    );
    let drive = v.mount_point.trim_end_matches('\\');
    assert!(
        v.name.ends_with(&format!("({drive})")),
        "{} names its drive {drive}",
        v.name
    );
}

#[test]
fn a_missing_path_is_an_error_not_a_guess() {
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("not-here");
    match volume_info(&missing) {
        Err(VolumeError::Io { path, source }) => {
            assert_eq!(path, missing);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected a not-found error, got {other:?}"),
    }
}

/// Read-only: asks the OS about the system drive's size and name, nothing more.
#[test]
fn reads_the_system_volume() {
    let v = system_volume().expect("system volume");
    assert!(!v.name.trim().is_empty(), "{v:?}");
    assert!(
        v.total_bytes > 0 && v.used_bytes > 0 && v.used_bytes <= v.total_bytes,
        "{v:?}"
    );
    #[cfg(target_os = "macos")]
    assert_eq!(v.mount_point, "/");
}

#[test]
fn names_this_machine() {
    assert!(!machine_name().trim().is_empty());
}

/// Read-only: lists the local volumes, the system volume first.
#[test]
fn lists_local_volumes_with_the_system_volume_first() {
    let volumes = list_volumes();
    let system = system_volume().expect("system volume");
    assert_eq!(
        volumes.first().map(|v| v.mount_point.as_str()),
        Some(system.mount_point.as_str()),
        "{volumes:?}"
    );
    for v in &volumes {
        assert!(v.total_bytes > 0 && v.used_bytes <= v.total_bytes, "{v:?}");
        assert!(!v.name.trim().is_empty(), "{v:?}");
    }
    let mut mounts: Vec<_> = volumes.iter().map(|v| &v.mount_point).collect();
    mounts.dedup();
    assert_eq!(mounts.len(), volumes.len(), "each volume once");
}
