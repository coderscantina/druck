//! Writing a file whole or not at all: a temporary file in the same folder, fsync, rename. Keeps the file's permissions
//! and writes through a symlink to its target. Holds no gpui types, and runs on any thread.
//!
//! [`write_if`] only replaces what it expects to find, so a save never overwrites a file another program wrote since
//! the last read. On macOS the final step swaps the two files atomically and checks what it took away, which closes the
//! moment between the check and the rename: a write landing in it is put back and reported.

use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const TEMPORARY_END: &str = ".druck-tmp";
/// A temporary file this old belongs to no running save. It was left by a crash.
const LEFTOVER_AGE: Duration = Duration::from_secs(3600);

/// What a file's bytes hash to. Recognizes the app's own writes among a folder's changes.
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Whether a path is a temporary file of this module.
pub fn is_temporary(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(TEMPORARY_END))
}

/// What a conditional write did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Written,
    /// The file did not hold what was expected, and it still holds this. `None` means no file. Nothing was written.
    Changed(Option<Vec<u8>>),
}

/// Replaces `path` with `bytes` if it holds content hashing to `expected`, or is absent for `None`.
pub fn write_if(path: &Path, bytes: &[u8], expected: Option<u64>) -> io::Result<Outcome> {
    let target = real_path(path);
    let current = read_if_there(&target)?;
    if !holds(current.as_deref(), expected) {
        return Ok(Outcome::Changed(current));
    }
    let temporary = temporary_file(&target, bytes, true)?;
    let placed = place(&temporary, &target, expected);
    if placed.is_err() || !matches!(placed, Ok(Outcome::Written)) {
        fs::remove_file(&temporary).ok();
    }
    if matches!(placed, Ok(Outcome::Written)) {
        sync_folder(&target);
    }
    placed
}

/// Replaces `path` with `bytes`, whatever it holds now.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = real_path(path);
    let temporary = temporary_file(&target, bytes, true)?;
    rename_or_clean(&temporary, &target)?;
    sync_folder(&target);
    Ok(())
}

/// Like [`write`], without waiting for the disk. Survives the app's crash, not the machine's. For recovery copies, which
/// are written often and only ever needed after a crash.
pub fn write_quick(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = real_path(path);
    let temporary = temporary_file(&target, bytes, false)?;
    rename_or_clean(&temporary, &target)
}

/// Removes the temporary files a crash left in a folder.
pub fn remove_leftovers(folder: &Path) {
    for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        let old = (entry.metadata().and_then(|meta| meta.modified()).ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > LEFTOVER_AGE);
        if old && is_temporary(&path) {
            fs::remove_file(path).ok();
        }
    }
}

fn holds(current: Option<&[u8]>, expected: Option<u64>) -> bool {
    current.map(content_hash) == expected
}

fn read_if_there(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// The file a write lands in: through a symlink to its target, also a target that does not exist yet.
fn real_path(path: &Path) -> PathBuf {
    if let Ok(real) = fs::canonicalize(path) {
        return real;
    }
    match fs::read_link(path) {
        Ok(link) => path.parent().unwrap_or(Path::new("")).join(link),
        Err(_) => path.to_owned(),
    }
}

/// Writes the bytes to a temporary file beside `target`, with `target`'s permissions. A read-only target refuses here,
/// because a rename would replace it regardless.
fn temporary_file(target: &Path, bytes: &[u8], sync: bool) -> io::Result<PathBuf> {
    let permissions = match fs::metadata(target) {
        Ok(metadata) if metadata.permissions().readonly() => {
            return Err(io::Error::new(ErrorKind::PermissionDenied, "the file is read-only"));
        }
        Ok(metadata) => Some(metadata.permissions()),
        Err(_) => None,
    };
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let temporary = target.with_file_name(format!(".{name}.{}{TEMPORARY_END}", std::process::id()));
    let written = (|| {
        let mut file = File::create(&temporary)?;
        file.write_all(bytes)?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        if sync {
            file.sync_all()?;
        }
        Ok(())
    })();
    match written {
        Ok(()) => Ok(temporary),
        Err(error) => {
            fs::remove_file(&temporary).ok();
            Err(error)
        }
    }
}

fn rename_or_clean(temporary: &Path, target: &Path) -> io::Result<()> {
    fs::rename(temporary, target).inspect_err(|_| {
        fs::remove_file(temporary).ok();
    })
}

/// Makes the rename itself survive a power cut. Some file systems cannot sync a folder, and the file's bytes are safe
/// either way, so a failure here is not a failed save.
fn sync_folder(target: &Path) {
    if let Some(folder) = target.parent()
        && let Ok(folder) = File::open(folder)
    {
        folder.sync_all().ok();
    }
}

#[cfg(target_os = "macos")]
fn place(temporary: &Path, target: &Path, expected: Option<u64>) -> io::Result<Outcome> {
    let flag = if expected.is_some() {
        libc::RENAME_SWAP
    } else {
        libc::RENAME_EXCL
    };
    match rename_with(temporary, target, flag) {
        Ok(()) if expected.is_none() => Ok(Outcome::Written),
        Ok(()) => {
            let displaced = fs::read(temporary)?;
            if holds(Some(&displaced), expected) {
                fs::remove_file(temporary).ok();
                return Ok(Outcome::Written);
            }
            // Another program wrote in the moment since the check. Its text goes back, this save is reported as not done.
            fs::rename(temporary, target)?;
            Ok(Outcome::Changed(Some(displaced)))
        }
        Err(error) if error.kind() == ErrorKind::NotFound && expected.is_some() => Ok(Outcome::Changed(None)),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(Outcome::Changed(read_if_there(target)?)),
        // A file system without the swap, such as a network share. The check above is all there is.
        Err(error) if matches!(error.raw_os_error(), Some(libc::ENOTSUP | libc::EINVAL)) => {
            fs::rename(temporary, target).map(|()| Outcome::Written)
        }
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "macos")]
fn rename_with(from: &Path, to: &Path, flag: libc::c_uint) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c_path = |path: &Path| {
        CString::new(path.as_os_str().as_bytes()).map_err(|error| io::Error::new(ErrorKind::InvalidInput, error))
    };
    let (from, to) = (c_path(from)?, c_path(to)?);
    // Both paths are valid C strings for the length of the call.
    if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), flag) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "macos"))]
fn place(temporary: &Path, target: &Path, _expected: Option<u64>) -> io::Result<Outcome> {
    fs::rename(temporary, target).map(|()| Outcome::Written)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("druck-atomic-{name}-{}", std::process::id()));
            fs::remove_dir_all(&dir).ok();
            fs::create_dir_all(&dir).expect("temporary folder");
            Self(dir)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    fn hash(text: &str) -> Option<u64> {
        Some(content_hash(text.as_bytes()))
    }

    #[test]
    fn a_write_replaces_the_file_keeps_its_mode_and_leaves_nothing_beside_it() {
        let folder = Folder::new("mode");
        let path = folder.0.join("a.md");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        assert_eq!(write_if(&path, b"new", hash("old")).unwrap(), Outcome::Written);
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o640);
        assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 1);
    }

    #[test]
    fn a_write_goes_through_a_symlink_and_keeps_it() {
        let folder = Folder::new("link");
        let real = folder.0.join("real.md");
        let link = folder.0.join("link.md");
        fs::write(&real, "old").unwrap();
        symlink(&real, &link).unwrap();
        assert_eq!(write_if(&link, b"new", hash("old")).unwrap(), Outcome::Written);
        assert_eq!(fs::read_to_string(&real).unwrap(), "new");
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }

    #[test]
    fn a_file_that_changed_is_left_alone_and_handed_back() {
        let folder = Folder::new("changed");
        let path = folder.0.join("a.md");
        fs::write(&path, "theirs").unwrap();
        assert_eq!(
            write_if(&path, b"mine", hash("base")).unwrap(),
            Outcome::Changed(Some(b"theirs".to_vec()))
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "theirs");
        assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 1);
    }

    #[test]
    fn a_file_that_went_or_came_is_reported() {
        let folder = Folder::new("gone");
        let path = folder.0.join("a.md");
        assert_eq!(write_if(&path, b"mine", hash("base")).unwrap(), Outcome::Changed(None));
        assert!(!path.exists());
        assert_eq!(write_if(&path, b"mine", None).unwrap(), Outcome::Written);
        assert_eq!(
            write_if(&path, b"again", None).unwrap(),
            Outcome::Changed(Some(b"mine".to_vec()))
        );
    }

    #[test]
    fn a_read_only_file_refuses_and_keeps_its_text() {
        let folder = Folder::new("read-only");
        let path = folder.0.join("a.md");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let error = write_if(&path, b"new", hash("old")).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
        assert_eq!(fs::read_to_string(&path).unwrap(), "old");
        assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 1);
    }

    #[test]
    fn a_missing_folder_is_an_error() {
        let folder = Folder::new("no-folder");
        let path = folder.0.join("gone").join("a.md");
        assert!(write_if(&path, b"new", None).is_err());
    }

    #[test]
    fn leftovers_go_but_not_a_save_still_running() {
        let folder = Folder::new("leftovers");
        let fresh = folder.0.join(format!(".a.md.1{TEMPORARY_END}"));
        fs::write(&fresh, "x").unwrap();
        remove_leftovers(&folder.0);
        assert!(fresh.exists());
        assert!(is_temporary(&fresh));
        assert!(!is_temporary(&folder.0.join("a.md")));
    }
}
