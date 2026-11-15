//! Linux binary startup only: keep an AppImage's GIO modules on its own
//! runtime libraries, while retaining the bundled GnuTLS backend.

use std::env;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct MissingBundledGioTls {
    app_dir: PathBuf,
}

impl fmt::Display for MissingBundledGioTls {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "impossible de démarrer cette AppImage : aucun module TLS GIO embarqué valide \
             (libgiognutls.so) dans {}. Réinstalle une AppImage officielle complète.",
            self.app_dir.display()
        )
    }
}

impl std::error::Error for MissingBundledGioTls {}

/// Configure GIO only when APPDIR is corroborated by this executable.
///
/// # Safety
/// Call once, at the start of main, before GTK or any application threads.
/// Environment mutation is exposed as safe in Rust edition 2021, but on Unix
/// concurrent readers (including native libraries) would make it unsafe.
pub unsafe fn initialize() -> Result<(), MissingBundledGioTls> {
    let app_dir = env::var_os("APPDIR");
    let executable = env::current_exe().ok();
    let extra_modules = env::var_os("GIO_EXTRA_MODULES");
    if let Some(modules) = select_gio_module_dir(
        app_dir.as_deref().map(Path::new),
        executable.as_deref(),
        extra_modules.as_deref(),
    )? {
        // Never combine bundled GLib with the host's GIO modules. Both paths
        // name the verified bundled directory; TLS remains available there.
        env::set_var("GIO_MODULE_DIR", &modules);
        env::set_var("GIO_EXTRA_MODULES", &modules);
    }
    Ok(())
}

/// No environment changes here, so selection can be tested without racing
/// the process-wide environment in the parallel Rust test runner.
fn select_gio_module_dir(
    app_dir: Option<&Path>,
    executable: Option<&Path>,
    extra_modules: Option<&OsStr>,
) -> Result<Option<PathBuf>, MissingBundledGioTls> {
    let Some(app_dir) = app_dir.and_then(|path| path.canonicalize().ok()) else {
        return Ok(None);
    };
    let Some(executable) = executable.and_then(|path| path.canonicalize().ok()) else {
        return Ok(None);
    };
    if !app_dir.is_dir()
        || !executable.is_file()
        || !executable
            .parent()
            .is_some_and(|parent| parent.starts_with(app_dir.join("usr/bin")))
    {
        // Native packages, dev builds and an unrelated/inherited APPDIR keep
        // their existing runtime environment unchanged.
        return Ok(None);
    }

    // linuxdeploy's GTK hook knows the packaged architecture-specific path.
    // Treat every entry as a candidate, never trust a host path it inherited.
    if let Some(extra_modules) = extra_modules {
        for candidate in env::split_paths(extra_modules) {
            if let Some(modules) = bundled_tls_modules(&app_dir, &candidate) {
                return Ok(Some(modules));
            }
        }
    }

    let lib_dir = app_dir.join("usr/lib");
    if let Some(modules) = bundled_tls_modules(&app_dir, &lib_dir.join("gio/modules")) {
        return Ok(Some(modules));
    }
    if let Ok(entries) = fs::read_dir(&lib_dir) {
        let mut candidates: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("gio/modules"))
            .collect();
        candidates.sort();
        for candidate in candidates {
            if let Some(modules) = bundled_tls_modules(&app_dir, &candidate) {
                return Ok(Some(modules));
            }
        }
    }

    // A recognized but incomplete AppImage must not silently lose TLS or
    // fall back to incompatible host modules. Fail before starting GTK.
    Err(MissingBundledGioTls { app_dir })
}

fn bundled_tls_modules(app_dir: &Path, candidate: &Path) -> Option<PathBuf> {
    let directory = candidate.canonicalize().ok()?;
    if !directory.starts_with(app_dir) || !directory.is_dir() {
        return None;
    }
    let tls_module = directory.join("libgiognutls.so").canonicalize().ok()?;
    if !tls_module.starts_with(app_dir) || !tls_module.is_file() {
        return None;
    }
    // GIO can load other modules from this same directory as well. A bundled
    // TLS module must not make a neighbouring symlink to a host plugin safe.
    for entry in fs::read_dir(&directory).ok()? {
        let path = entry.ok()?.path();
        if path.extension() == Some(OsStr::new("so")) {
            let module = path.canonicalize().ok()?;
            if !module.starts_with(app_dir) || !module.is_file() {
                return None;
            }
        }
    }
    Some(directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    struct AppDir {
        root: TempDir,
        path: PathBuf,
        executable: PathBuf,
    }

    impl AppDir {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("Rustory.AppDir");
            let executable = path.join("usr/bin/rustory");
            fs::create_dir_all(executable.parent().unwrap()).unwrap();
            fs::write(&executable, b"test executable").unwrap();
            Self {
                root,
                path,
                executable,
            }
        }

        fn select(&self, extra: Option<&OsStr>) -> Result<Option<PathBuf>, MissingBundledGioTls> {
            select_gio_module_dir(Some(&self.path), Some(&self.executable), extra)
        }
    }

    fn write_tls_module(directory: &Path) -> PathBuf {
        fs::create_dir_all(directory).unwrap();
        fs::write(directory.join("libgiognutls.so"), b"test TLS module").unwrap();
        directory.canonicalize().unwrap()
    }

    #[test]
    fn selects_bundled_gnutls_for_a_verified_appdir() {
        let app = AppDir::new();
        let modules = write_tls_module(&app.path.join("usr/lib/gio/modules"));
        assert_eq!(app.select(None).unwrap(), Some(modules));
    }

    #[test]
    fn prefers_the_verified_gtk_hook_path_over_fallback_paths() {
        let app = AppDir::new();
        write_tls_module(&app.path.join("usr/lib/gio/modules"));
        let preferred = write_tls_module(&app.path.join("gtk-runtime/gio/modules"));
        assert_eq!(
            app.select(Some(preferred.as_os_str())).unwrap(),
            Some(preferred)
        );
    }

    #[test]
    fn discovers_architecture_specific_bundled_modules() {
        let app = AppDir::new();
        let modules = write_tls_module(&app.path.join("usr/lib/x86_64-linux-gnu/gio/modules"));
        assert_eq!(app.select(None).unwrap(), Some(modules));
    }

    #[test]
    fn leaves_native_execution_and_unrelated_appdir_unchanged() {
        let app = AppDir::new();
        let native = app.root.path().join("native-rustory");
        fs::write(&native, b"native executable").unwrap();
        assert!(select_gio_module_dir(None, Some(&native), None)
            .unwrap()
            .is_none());
        assert!(select_gio_module_dir(Some(&app.path), Some(&native), None)
            .unwrap()
            .is_none());
        assert!(select_gio_module_dir(
            Some(&app.root.path().join("missing.AppDir")),
            Some(&app.executable),
            None
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn requires_a_real_executable_inside_the_canonical_appdir() {
        let app = AppDir::new();
        assert!(
            select_gio_module_dir(Some(&app.path), Some(&app.path.join("usr/bin")), None)
                .unwrap()
                .is_none()
        );
        fs::remove_file(&app.executable).unwrap();
        let outside = app.root.path().join("outside-executable");
        fs::write(&outside, b"outside executable").unwrap();
        symlink(&outside, &app.executable).unwrap();
        assert!(app.select(None).unwrap().is_none());
    }

    #[test]
    fn ignores_host_module_paths_and_keeps_only_the_bundled_candidate() {
        let app = AppDir::new();
        let host = write_tls_module(&app.root.path().join("host/gio/modules"));
        let bundled = write_tls_module(&app.path.join("usr/lib/gio/modules"));
        let extra = env::join_paths([host, bundled.clone()]).unwrap();
        assert_eq!(app.select(Some(&extra)).unwrap(), Some(bundled));
    }

    #[test]
    fn refuses_module_directories_symlinked_outside_the_appdir() {
        let app = AppDir::new();
        let host = write_tls_module(&app.root.path().join("host/gio/modules"));
        let linked = app.path.join("usr/lib/gio/modules");
        fs::create_dir_all(linked.parent().unwrap()).unwrap();
        symlink(host, &linked).unwrap();
        assert!(app.select(Some(linked.as_os_str())).is_err());
    }

    #[test]
    fn refuses_a_tls_module_symlinked_to_the_host() {
        let app = AppDir::new();
        let host = write_tls_module(&app.root.path().join("host/gio/modules"));
        let bundled = app.path.join("usr/lib/gio/modules");
        fs::create_dir_all(&bundled).unwrap();
        symlink(
            host.join("libgiognutls.so"),
            bundled.join("libgiognutls.so"),
        )
        .unwrap();
        assert!(app.select(None).is_err());
    }

    #[test]
    fn refuses_a_host_plugin_alongside_a_valid_bundled_tls_module() {
        let app = AppDir::new();
        let bundled = write_tls_module(&app.path.join("usr/lib/gio/modules"));
        let host_plugin = app.root.path().join("libgvfsdbus.so");
        fs::write(&host_plugin, b"host plugin").unwrap();
        symlink(&host_plugin, bundled.join("libgvfsdbus.so")).unwrap();
        assert!(app.select(None).is_err());
    }

    #[test]
    fn accepts_internal_symlinks_and_canonicalizes_the_appdir() {
        let app = AppDir::new();
        let actual = write_tls_module(&app.path.join("gtk-runtime/gio/modules"));
        let bundled = app.path.join("usr/lib/gio/modules");
        fs::create_dir_all(&bundled).unwrap();
        symlink(
            actual.join("libgiognutls.so"),
            bundled.join("libgiognutls.so"),
        )
        .unwrap();
        let app_alias = app.root.path().join("app-alias");
        symlink(&app.path, &app_alias).unwrap();
        assert_eq!(
            select_gio_module_dir(Some(&app_alias), Some(&app.executable), None).unwrap(),
            Some(bundled.canonicalize().unwrap())
        );
    }

    #[test]
    fn fails_explicitly_without_a_bundled_tls_module_instead_of_disabling_tls() {
        let app = AppDir::new();
        let modules = app.path.join("usr/lib/gio/modules");
        fs::create_dir_all(&modules).unwrap();
        fs::write(modules.join("libgiolibproxy.so"), b"not a TLS module").unwrap();
        let host = write_tls_module(&app.root.path().join("host/gio/modules"));
        let error = app.select(Some(host.as_os_str())).unwrap_err();
        assert!(error.to_string().contains("libgiognutls.so"));
        assert!(error.to_string().contains("Réinstalle"));
    }
}
