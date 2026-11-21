//! The one door to the native file dialogs.
//!
//! Every command that lets the user pick or save a file builds its dialog
//! with [`file_dialog`], so the starting-directory rule of
//! [`dialog_start_directory`] holds for all of them — and for the next one
//! (a test below refuses a dialog opened any other way).

use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};

use crate::infrastructure::filesystem::dialog_start_directory;

/// Start building a native file dialog.
///
/// From an AppImage the dialog opens in the directory the AppImage was
/// launched from, else in the home directory — never in the image's own
/// mount. Everywhere else the builder is returned untouched and the native
/// dialog decides.
pub fn file_dialog<R: Runtime>(app: &AppHandle<R>) -> FileDialogBuilder<R> {
    let dialog = app.dialog().file();
    match dialog_start_directory::start_directory() {
        Some(directory) => dialog.set_directory(directory),
        None => dialog,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    fn rust_sources(directory: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("read a source directory") {
            let path = entry.expect("read a source entry").path();
            if path.is_dir() {
                rust_sources(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }

    /// A dialog built straight from the plugin starts wherever the toolkit
    /// decides — inside the image's read-only mount for an AppImage. This
    /// module is the only place allowed to reach the plugin's file dialog.
    #[test]
    fn every_native_file_dialog_is_built_through_this_module() {
        let sources_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let this_module = sources_root.join("commands").join("file_dialog.rs");
        let mut sources = Vec::new();
        rust_sources(&sources_root, &mut sources);
        assert!(
            sources.contains(&this_module),
            "the scan must cover the commands"
        );

        for source in sources.into_iter().filter(|path| *path != this_module) {
            // rustfmt is free to break the call chain across lines.
            let code: String = fs::read_to_string(&source)
                .expect("read a source file")
                .split_whitespace()
                .collect();
            assert!(
                !code.contains(".dialog().file()"),
                "{} opens a native file dialog without `file_dialog`",
                source.display()
            );
        }
    }
}
