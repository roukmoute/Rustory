//! Starting directory of the native file dialogs when Rustory runs from an
//! AppImage.
//!
//! GTK opens a file dialog in the working directory of the process when the
//! application names none. The AppImage launcher (`AppRun.wrapped`) moves
//! that working directory to `$APPDIR/usr` — the image's own mount — before
//! it starts Rustory, so a save dialog validated as it opened pointed at a
//! place nobody can write to.
//!
//! The working directory itself must stay where the launcher put it: the
//! bundled WebKit library is patched with paths relative to it and starts
//! its helper processes through them for the whole life of the process. The
//! directory is therefore handed to each dialog instead.
//!
//! Nothing is imposed anywhere else: a system package, a development build,
//! Windows and macOS keep the behavior of their native dialogs.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// What the process knows about the way it was launched — the inputs of
/// [`select_start_directory`]. Kept apart from the environment so the rule
/// is tested on plain values, never on process-wide state.
#[derive(Debug, Clone, Copy, Default)]
pub struct LaunchContext<'a> {
    /// `$APPDIR`: root of the mounted (or extracted) AppImage.
    pub app_dir: Option<&'a Path>,
    /// Working directory of the process.
    pub working_dir: Option<&'a Path>,
    /// Directory of the last choice made in a file dialog of this session.
    pub last_used_dir: Option<&'a Path>,
    /// `$OWD`: the directory the AppImage was launched from, saved by the
    /// AppImage runtime before the launcher moved the process.
    pub launch_dir: Option<&'a Path>,
    /// `$HOME`.
    pub home_dir: Option<&'a Path>,
}

/// Choose the directory a native file dialog must start in, or `None` when
/// the toolkit is left to decide.
///
/// A directory is imposed only when the working directory sits inside
/// `$APPDIR`: that is the launcher's doing, and what GTK would open is the
/// image's own mount. An `APPDIR` merely inherited from another program
/// leaves the working directory alone, and so does this rule.
///
/// The candidates are tried in order — the directory last used in this
/// session, the directory the AppImage was launched from, the home
/// directory — and the first usable one wins (see [`is_usable_outside`]).
/// Without any, nothing is imposed.
pub fn select_start_directory(context: &LaunchContext<'_>) -> Option<PathBuf> {
    let app_dir = context.app_dir?.canonicalize().ok()?;
    let working_dir = context.working_dir?.canonicalize().ok()?;
    if !working_dir.starts_with(&app_dir) {
        return None;
    }
    [context.last_used_dir, context.launch_dir, context.home_dir]
        .into_iter()
        .flatten()
        .find(|candidate| is_usable_outside(candidate, &app_dir))
        .map(Path::to_path_buf)
}

/// A candidate is usable when it is an absolute path to an existing
/// directory that lies outside the AppDir. It is judged on its resolved
/// form — a link into the AppDir is still the AppDir — but handed to the
/// dialog as the user spells it.
fn is_usable_outside(candidate: &Path, app_dir: &Path) -> bool {
    // The dialog layer forwards a starting directory as UTF-8 text and
    // silently drops any other path, which would bring the AppDir back.
    if !candidate.is_absolute() || candidate.to_str().is_none() {
        return false;
    }
    candidate
        .canonicalize()
        .is_ok_and(|resolved| resolved.is_dir() && !resolved.starts_with(app_dir))
}

/// Session memory of the directory where the user last chose a file or a
/// folder in a native dialog.
///
/// Imposing the same directory on every dialog would send the user back to
/// it each time; this memory lets the next dialog reopen where the previous
/// one ended. It lives and dies with the process: nothing is written to
/// disk, and it is read only where a directory is imposed.
///
/// Instantiable so unit tests work on local instances — the process-wide
/// [`LAST_USED_DIRECTORY`] is consumed only by the commands.
#[derive(Debug, Default)]
pub struct LastUsedDirectory {
    directory: Mutex<Option<PathBuf>>,
}

/// Process-wide memory consumed by the commands that open a file dialog.
pub static LAST_USED_DIRECTORY: LastUsedDirectory = LastUsedDirectory::new();

impl LastUsedDirectory {
    pub const fn new() -> Self {
        Self {
            directory: Mutex::new(None),
        }
    }

    /// Remember the directory that holds `chosen`, the file or the folder
    /// the user just picked or saved. A choice without a parent directory
    /// (a filesystem root) keeps the previous memory.
    pub fn remember(&self, chosen: &Path) {
        let Some(directory) = chosen.parent().filter(|dir| !dir.as_os_str().is_empty()) else {
            return;
        };
        *self.lock() = Some(directory.to_path_buf());
    }

    /// The remembered directory, if any. It may have vanished since: the
    /// selection checks it again before every use.
    pub fn get(&self) -> Option<PathBuf> {
        self.lock().clone()
    }

    fn lock(&self) -> MutexGuard<'_, Option<PathBuf>> {
        // The protected value is a plain slot swap, always left coherent:
        // recover the guard of a poisoned mutex.
        self.directory
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The directory the next native file dialog of this process must start
/// in, or `None` when the toolkit decides — always the case outside a
/// Linux AppImage.
pub fn start_directory() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        // `APPDIR`, `OWD` and the launcher's working directory are AppImage
        // conventions; the Windows and macOS dialogs choose and remember
        // their directory by themselves.
        return None;
    }
    let app_dir = env::var_os("APPDIR").map(PathBuf::from);
    let working_dir = env::current_dir().ok();
    let last_used_dir = LAST_USED_DIRECTORY.get();
    let launch_dir = env::var_os("OWD").map(PathBuf::from);
    let home_dir = env::var_os("HOME").map(PathBuf::from);
    select_start_directory(&LaunchContext {
        app_dir: app_dir.as_deref(),
        working_dir: working_dir.as_deref(),
        last_used_dir: last_used_dir.as_deref(),
        launch_dir: launch_dir.as_deref(),
        home_dir: home_dir.as_deref(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// A launched AppImage: the launcher left the process in `usr/`, the
    /// user launched it from `launch/` and their home is `home/`.
    struct Launched {
        root: TempDir,
        app_dir: PathBuf,
        working_dir: PathBuf,
        launch_dir: PathBuf,
        home_dir: PathBuf,
    }

    impl Launched {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let app_dir = root.path().join("Rustory.AppDir");
            let working_dir = app_dir.join("usr");
            let launch_dir = root.path().join("launch");
            let home_dir = root.path().join("home");
            for directory in [&working_dir, &launch_dir, &home_dir] {
                fs::create_dir_all(directory).unwrap();
            }
            Self {
                root,
                app_dir,
                working_dir,
                launch_dir,
                home_dir,
            }
        }

        fn context(&self) -> LaunchContext<'_> {
            LaunchContext {
                app_dir: Some(&self.app_dir),
                working_dir: Some(&self.working_dir),
                last_used_dir: None,
                launch_dir: Some(&self.launch_dir),
                home_dir: Some(&self.home_dir),
            }
        }

        fn directory(&self, name: &str) -> PathBuf {
            let directory = self.root.path().join(name);
            fs::create_dir_all(&directory).unwrap();
            directory
        }
    }

    #[test]
    fn starts_in_the_launch_directory_of_an_appimage() {
        let app = Launched::new();
        assert_eq!(
            select_start_directory(&app.context()),
            Some(app.launch_dir.clone())
        );
    }

    #[test]
    fn imposes_nothing_without_an_appdir() {
        let app = Launched::new();
        let context = LaunchContext {
            app_dir: None,
            ..app.context()
        };
        assert_eq!(select_start_directory(&context), None);
    }

    #[test]
    fn imposes_nothing_when_the_appdir_does_not_exist() {
        let app = Launched::new();
        let missing = app.root.path().join("missing.AppDir");
        let context = LaunchContext {
            app_dir: Some(&missing),
            ..app.context()
        };
        assert_eq!(select_start_directory(&context), None);
    }

    #[test]
    fn imposes_nothing_when_the_working_directory_is_outside_the_appdir() {
        // A system package or a development build started from a program
        // that is itself an AppImage inherits its `APPDIR` and its `OWD`.
        let app = Launched::new();
        let elsewhere = app.directory("elsewhere");
        let context = LaunchContext {
            working_dir: Some(&elsewhere),
            ..app.context()
        };
        assert_eq!(select_start_directory(&context), None);
    }

    #[test]
    fn imposes_nothing_when_the_working_directory_is_unknown() {
        let app = Launched::new();
        let context = LaunchContext {
            working_dir: None,
            ..app.context()
        };
        assert_eq!(select_start_directory(&context), None);
    }

    #[test]
    fn the_appdir_root_itself_counts_as_inside() {
        let app = Launched::new();
        let context = LaunchContext {
            working_dir: Some(&app.app_dir),
            ..app.context()
        };
        assert_eq!(
            select_start_directory(&context),
            Some(app.launch_dir.clone())
        );
    }

    #[test]
    fn a_sibling_sharing_the_appdir_name_prefix_is_outside() {
        // Containment is decided on whole path components, never on text.
        let app = Launched::new();
        let sibling = app.directory("Rustory.AppDir-notes");
        let as_working_dir = LaunchContext {
            working_dir: Some(&sibling),
            ..app.context()
        };
        assert_eq!(select_start_directory(&as_working_dir), None);
        let as_launch_dir = LaunchContext {
            launch_dir: Some(&sibling),
            ..app.context()
        };
        assert_eq!(select_start_directory(&as_launch_dir), Some(sibling));
    }

    #[test]
    fn falls_back_to_the_home_directory_without_a_usable_launch_directory() {
        let app = Launched::new();
        let file = app.root.path().join("a-file");
        fs::write(&file, b"not a directory").unwrap();
        let unusable = [
            None,
            Some(app.root.path().join("vanished")),
            Some(PathBuf::from("relative/launch")),
            Some(PathBuf::new()),
            Some(file),
            // Launched from inside the image, or restarted by a previous
            // copy of itself: the launch directory is the mount again.
            Some(app.working_dir.clone()),
            Some(app.app_dir.clone()),
        ];
        for launch_dir in unusable {
            let context = LaunchContext {
                launch_dir: launch_dir.as_deref(),
                ..app.context()
            };
            assert_eq!(
                select_start_directory(&context),
                Some(app.home_dir.clone()),
                "launch directory {launch_dir:?}"
            );
        }
    }

    #[test]
    fn imposes_nothing_when_no_candidate_is_usable() {
        let app = Launched::new();
        let vanished = app.root.path().join("vanished");
        let context = LaunchContext {
            last_used_dir: Some(&vanished),
            launch_dir: Some(&app.working_dir),
            home_dir: None,
            ..app.context()
        };
        assert_eq!(select_start_directory(&context), None);
    }

    #[test]
    fn prefers_the_directory_last_used_in_the_session() {
        let app = Launched::new();
        let documents = app.directory("documents");
        let context = LaunchContext {
            last_used_dir: Some(&documents),
            ..app.context()
        };
        assert_eq!(select_start_directory(&context), Some(documents));
    }

    #[test]
    fn a_last_used_directory_is_checked_again_before_every_use() {
        let app = Launched::new();
        // An unplugged volume, a deleted folder.
        let unplugged = app.root.path().join("unplugged");
        let vanished = LaunchContext {
            last_used_dir: Some(&unplugged),
            ..app.context()
        };
        assert_eq!(
            select_start_directory(&vanished),
            Some(app.launch_dir.clone())
        );
        // The user browsed into the image and picked a file there.
        let inside = LaunchContext {
            last_used_dir: Some(&app.working_dir),
            ..app.context()
        };
        assert_eq!(
            select_start_directory(&inside),
            Some(app.launch_dir.clone())
        );
    }

    #[test]
    fn the_session_memory_never_imposes_a_directory_by_itself() {
        let app = Launched::new();
        let documents = app.directory("documents");
        let context = LaunchContext {
            app_dir: None,
            last_used_dir: Some(&documents),
            ..app.context()
        };
        assert_eq!(select_start_directory(&context), None);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_candidate_linked_into_the_appdir() {
        let app = Launched::new();
        let link = app.root.path().join("link-into-the-image");
        std::os::unix::fs::symlink(&app.working_dir, &link).unwrap();
        let context = LaunchContext {
            launch_dir: Some(&link),
            ..app.context()
        };
        assert_eq!(select_start_directory(&context), Some(app.home_dir.clone()));
    }

    #[cfg(unix)]
    #[test]
    fn recognizes_an_appdir_named_through_a_link() {
        // The mount point can be named through a linked temporary
        // directory while the kernel reports the resolved working directory.
        let app = Launched::new();
        let alias = app.root.path().join("appdir-alias");
        std::os::unix::fs::symlink(&app.app_dir, &alias).unwrap();
        let context = LaunchContext {
            app_dir: Some(&alias),
            launch_dir: Some(&app.working_dir),
            ..app.context()
        };
        assert_eq!(select_start_directory(&context), Some(app.home_dir.clone()));
    }

    #[cfg(unix)]
    #[test]
    fn hands_over_the_directory_as_the_user_spells_it() {
        // A home directory reached through a link stays recognizable as
        // the home directory by the dialog.
        let app = Launched::new();
        let spelled = app.root.path().join("home-link");
        std::os::unix::fs::symlink(&app.home_dir, &spelled).unwrap();
        let context = LaunchContext {
            launch_dir: None,
            home_dir: Some(&spelled),
            ..app.context()
        };
        assert_eq!(select_start_directory(&context), Some(spelled));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn skips_a_directory_the_dialog_could_not_be_given() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let app = Launched::new();
        let not_utf8 = app.root.path().join(OsStr::from_bytes(b"launch-\xff"));
        fs::create_dir(&not_utf8).unwrap();
        let context = LaunchContext {
            launch_dir: Some(&not_utf8),
            ..app.context()
        };
        assert_eq!(select_start_directory(&context), Some(app.home_dir.clone()));
    }

    #[test]
    fn remembers_the_directory_holding_the_choice() {
        let memory = LastUsedDirectory::new();
        assert_eq!(memory.get(), None);

        memory.remember(Path::new("/home/ana/Documents/histoire.zip"));
        assert_eq!(memory.get(), Some(PathBuf::from("/home/ana/Documents")));

        // A picked folder is remembered through the directory that lists
        // it, where its neighbours are.
        memory.remember(Path::new("/media/ana/cle/packs/le-loup"));
        assert_eq!(memory.get(), Some(PathBuf::from("/media/ana/cle/packs")));
    }

    #[test]
    fn a_choice_without_a_parent_directory_keeps_the_previous_memory() {
        let memory = LastUsedDirectory::new();
        memory.remember(Path::new("/home/ana/histoire.zip"));
        for rootless in ["/", "histoire.zip", ""] {
            memory.remember(Path::new(rootless));
            assert_eq!(
                memory.get(),
                Some(PathBuf::from("/home/ana")),
                "choice {rootless:?}"
            );
        }
    }
}
