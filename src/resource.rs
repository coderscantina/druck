//! Where resources come from. The engine reads every font, image, bibliography, theme, and chapter through a
//! [`Resolver`], so this module and the caller's own code are the only places that touch the machine.
//!
//! A resolver is shared by every job that uses it and may be called from several threads at once. A path does
//! not promise unchanged bytes: each job reads what it uses again and reuses preparation by content hash.

use std::collections::HashMap;
use std::fmt;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::io::ErrorKind;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[doc(hidden)]
pub use crate::config::source::normalize;
pub use crate::config::source::{Origin, Resource};
use crate::date::Date;
use crate::installed;
use crate::text::bundled;

/// Supplies the bytes of resources.
pub trait Resolver: Send + Sync {
    /// The bytes of `path` relative to `origin`. Errors are plain messages: the engine adds the resource, its
    /// origin, the property that named it, and the source location.
    fn read(&self, origin: &Origin, path: &str) -> Result<Bytes, ResolveError>;

    /// The files below `dir`, as paths relative to `origin` that [`Resolver::read`] takes. `None` where listing
    /// is not supported, which the engine treats as an empty folder.
    fn list(&self, _origin: &Origin, _dir: &str) -> Option<Vec<String>> {
        None
    }

    /// The faces of an installed family, for themes that name fonts without files. The default finds none.
    fn installed(&self, _family: &str) -> Result<Vec<InstalledFace>, ResolveError> {
        Ok(Vec::new())
    }
}

/// One face of an installed family. Its file is read with [`Origin::Installed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledFace {
    /// The font file, absolute.
    pub path: String,
    /// The face in a font collection, else 0.
    pub index: u32,
    pub weight: u16,
    pub italic: bool,
}

/// Why a resource could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// Nothing is there. The message says so in the resolver's own words.
    NotFound(String),
    /// Something is there and cannot be read.
    Unreadable(String),
    /// This resolver does not serve that kind of origin.
    Unsupported,
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(message) | Self::Unreadable(message) => f.write_str(message),
            Self::Unsupported => f.write_str("this resolver does not serve that origin"),
        }
    }
}

/// Immutable bytes with a content hash computed once. Cheap to clone.
#[derive(Clone)]
pub struct Bytes {
    data: krilla::Data,
    hash: u64,
}

impl Bytes {
    /// A file compiled into the binary. Its contents are fixed by its path, so it costs no hashing per job.
    pub(crate) fn bundled(path: &str, data: &'static [u8]) -> Self {
        Self {
            data: data.into(),
            hash: content_hash(path.as_bytes()),
        }
    }

    /// The hash that decides whether two reads may share preparation.
    pub fn hash(&self) -> u64 {
        self.hash
    }

    pub(crate) fn data(&self) -> krilla::Data {
        self.data.clone()
    }
}

/// A hash with a key of this process, so that no file can be made in advance to collide with another.
fn content_hash(bytes: &[u8]) -> u64 {
    static KEY: OnceLock<RandomState> = OnceLock::new();
    let mut hasher = KEY.get_or_init(RandomState::new).build_hasher();
    hasher.write(bytes);
    hasher.finish()
}

impl Deref for Bytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.data.as_ref()
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(data: Vec<u8>) -> Self {
        let hash = content_hash(&data);
        Self {
            data: data.into(),
            hash,
        }
    }
}

impl From<Arc<[u8]>> for Bytes {
    fn from(data: Arc<[u8]>) -> Self {
        let hash = content_hash(&data);
        let data: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(data);
        Self {
            data: data.into(),
            hash,
        }
    }
}

impl PartialEq for Bytes {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.len() == other.len()
    }
}

impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bytes({} bytes)", self.len())
    }
}

/// Reads files. This is the only reason the engine touches a disk, and only when a caller chooses it. Paths are
/// joined and normalized without touching the filesystem, so symbolic links stay as written.
#[derive(Default)]
pub struct FileResolver {
    fonts: OnceLock<fontdb::Database>,
}

impl FileResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// A resolver whose installed fonts are `fonts` instead of the system's.
    #[cfg(test)]
    pub(crate) fn with_fonts(fonts: fontdb::Database) -> Self {
        Self {
            fonts: OnceLock::from(fonts),
        }
    }

    /// The folder of named themes shared by all projects, if the platform has a config folder.
    pub fn shared_themes() -> Option<PathBuf> {
        config_root(
            cfg!(windows),
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            std::env::var_os("HOME").map(PathBuf::from),
            std::env::var_os("APPDATA").map(PathBuf::from),
        )
        .map(|root| root.join("druck").join("themes"))
    }
}

impl Resolver for FileResolver {
    fn read(&self, origin: &Origin, path: &str) -> Result<Bytes, ResolveError> {
        let file = origin.file(path).ok_or(ResolveError::Unsupported)?;
        match std::fs::read(file) {
            Ok(data) => Ok(data.into()),
            Err(e) if is_missing(e.kind()) => Err(ResolveError::NotFound(e.to_string())),
            Err(e) => Err(ResolveError::Unreadable(e.to_string())),
        }
    }

    fn list(&self, origin: &Origin, dir: &str) -> Option<Vec<String>> {
        let root = origin.file("")?;
        let mut files = Vec::new();
        let mut folders = vec![root.join(dir)];
        while let Some(folder) = folders.pop() {
            for path in std::fs::read_dir(&folder)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
            {
                if path.is_dir() {
                    folders.push(path);
                } else if let Some(relative) = path.strip_prefix(&root).ok().and_then(Path::to_str) {
                    files.push(relative.to_owned());
                }
            }
        }
        Some(files)
    }

    /// Scans the system's fonts on the first call only.
    fn installed(&self, family: &str) -> Result<Vec<InstalledFace>, ResolveError> {
        let fonts = self.fonts.get_or_init(|| {
            let mut fonts = fontdb::Database::new();
            fonts.load_system_fonts();
            fonts
        });
        Ok(installed::family_faces(fonts, family))
    }
}

/// Whether a read failed because no file is there. A folder is not a file.
fn is_missing(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::NotFound | ErrorKind::IsADirectory | ErrorKind::NotADirectory
    )
}

fn config_root(
    windows: bool,
    xdg: Option<PathBuf>,
    home: Option<PathBuf>,
    appdata: Option<PathBuf>,
) -> Option<PathBuf> {
    if windows {
        appdata.filter(|path| path.is_absolute())
    } else {
        xdg.filter(|path| path.is_absolute())
            .or_else(|| home.filter(|path| path.is_absolute()).map(|path| path.join(".config")))
    }
}

/// Serves files from memory. Nothing else is read, and installed fonts are not found.
#[derive(Default)]
pub struct MemoryResolver {
    files: HashMap<Origin, HashMap<String, Bytes>>,
}

impl MemoryResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a file. Its hash is computed here, once.
    pub fn insert(&mut self, origin: Origin, path: &str, data: impl Into<Arc<[u8]>>) {
        let bytes = Bytes::from(data.into());
        self.files.entry(origin).or_default().insert(memory_key(path), bytes);
    }
}

fn memory_key(path: &str) -> String {
    normalize(Path::new(path)).to_string_lossy().into_owned()
}

impl Resolver for MemoryResolver {
    fn read(&self, origin: &Origin, path: &str) -> Result<Bytes, ResolveError> {
        self.files
            .get(origin)
            .and_then(|files| files.get(&memory_key(path)))
            .cloned()
            .ok_or_else(|| ResolveError::NotFound("not in memory".to_owned()))
    }

    fn list(&self, origin: &Origin, dir: &str) -> Option<Vec<String>> {
        let files = self.files.get(origin)?;
        let below = |path: &&String| Path::new(path.as_str()).starts_with(dir);
        Some(files.keys().filter(below).cloned().collect())
    }
}

/// The resources one job has read, so that everything in the job sees the same bytes.
#[derive(Clone, Default)]
pub(crate) struct Files(HashMap<Resource, Bytes>);

impl Files {
    pub fn insert(&mut self, resource: Resource, data: Bytes) {
        self.0.insert(resource, data);
    }

    pub fn contains(&self, resource: &Resource) -> bool {
        self.0.contains_key(resource)
    }

    pub fn extend(&mut self, files: HashMap<Resource, Bytes>) {
        self.0.extend(files);
    }

    /// The bytes of `resource`. Bundled files are always there; others have to have been read.
    pub fn get(&self, resource: &Resource) -> Result<Bytes, String> {
        if resource.origin == Origin::Bundled {
            return bundled::font(&resource.path)
                .map(|data| Bytes::bundled(&resource.path, data))
                .ok_or_else(|| format!("unknown bundled font: {resource}"));
        }
        (self.0.get(resource).cloned()).ok_or_else(|| format!("resource not found: {resource}"))
    }
}

/// The UTC day of `SOURCE_DATE_EPOCH`, if it is set, which builds that must give the same bytes use to fix
/// the build date. A caller passes it as `Input::today`, and as `EpubOptions::modified`.
pub fn source_date() -> Result<Option<Date>, String> {
    let Ok(value) = std::env::var("SOURCE_DATE_EPOCH") else {
        return Ok(None);
    };
    let seconds = value
        .trim()
        .parse::<i64>()
        .map_err(|_| format!("SOURCE_DATE_EPOCH must be a whole number of seconds since 1970, not \"{value}\""))?;
    Ok(Some(Date::from_unix(seconds)))
}

/// The UTC day of the system clock.
pub fn clock_date() -> Result<Date, String> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "the system clock is set before 1970".to_owned())?;
    let seconds = i64::try_from(elapsed.as_secs()).map_err(|_| "the system clock is out of range".to_owned())?;
    Ok(Date::from_unix(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_absolute_os_config_roots_and_ignores_relative_xdg_values() {
        let root = std::env::temp_dir();
        let home = root.join("home");
        let xdg = root.join("xdg");
        let appdata = root.join("appdata");
        assert_eq!(
            config_root(false, Some(xdg.clone()), Some(home.clone()), None),
            Some(xdg)
        );
        assert_eq!(
            config_root(false, Some("relative".into()), Some(home.clone()), None),
            Some(home.join(".config"))
        );
        assert_eq!(
            config_root(true, None, Some(home), Some(appdata.clone())),
            Some(appdata)
        );
        assert_eq!(config_root(false, None, None, None), None);
    }

    #[test]
    fn a_memory_resolver_reads_by_origin_and_lists_below_a_folder() {
        let origin = Origin::Document("/book".into());
        let mut resolver = MemoryResolver::new();
        resolver.insert(origin.clone(), "./manuscript/one.md", b"one".as_slice());
        resolver.insert(origin.clone(), "other.md", b"other".as_slice());
        assert_eq!(&*resolver.read(&origin, "manuscript/one.md").unwrap(), b"one");
        assert!(matches!(
            resolver.read(&Origin::Theme("/book".into()), "other.md"),
            Err(ResolveError::NotFound(_))
        ));
        assert_eq!(resolver.list(&origin, "manuscript").unwrap(), ["manuscript/one.md"]);
    }

    #[test]
    fn the_same_bytes_hash_alike_however_they_arrive() {
        let bytes: Bytes = b"data".to_vec().into();
        let shared: Bytes = Arc::<[u8]>::from(b"data".as_slice()).into();
        let other: Bytes = b"datb".to_vec().into();
        assert_eq!(bytes.hash(), shared.hash());
        assert_ne!(bytes.hash(), other.hash());
    }
}
