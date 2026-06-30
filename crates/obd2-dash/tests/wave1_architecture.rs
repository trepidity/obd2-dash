use std::fs;
use std::path::{Path, PathBuf};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(dir, &mut files);
    files.sort();
    files
}

fn collect_rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read source directory") {
        let entry = entry.expect("read source entry");
        let path = entry.path();

        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

fn relative(path: &Path) -> String {
    path.strip_prefix(manifest_dir())
        .expect("test path under manifest")
        .display()
        .to_string()
}

#[test]
fn live_modules_do_not_fabricate_selected_profiles() {
    let src = manifest_dir().join("src");
    let profiles_dir = src.join("profiles");
    let lib_rs = src.join("lib.rs");

    for path in rust_files_under(&src) {
        if path == lib_rs || path.starts_with(&profiles_dir) {
            continue;
        }

        let text = fs::read_to_string(&path).expect("read source file");
        assert!(
            !text.contains("SelectedProfile::seal"),
            "{} names SelectedProfile::seal; only crate::profiles may mint selected-profile tokens",
            relative(&path)
        );
    }
}

#[test]
fn profile_registry_is_only_seal_call_site() {
    let profiles_dir = manifest_dir().join("src").join("profiles");
    let registry_rs = profiles_dir.join("registry.rs");
    let model_rs = profiles_dir.join("model.rs");

    for path in rust_files_under(&profiles_dir) {
        let text = fs::read_to_string(&path).expect("read profile source file");
        let count = text.match_indices("SelectedProfile::seal").count();
        let expected = if path == registry_rs {
            2
        } else if path == model_rs {
            4
        } else {
            0
        };
        assert_eq!(
            count,
            expected,
            "{} has unexpected SelectedProfile::seal call count",
            relative(&path)
        );
    }
}

#[test]
fn lib_declares_profiles() {
    let lib_rs = manifest_dir().join("src").join("lib.rs");
    let text = fs::read_to_string(&lib_rs).expect("read lib.rs");

    assert!(text.contains("pub mod profiles;"));
}
