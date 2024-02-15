// A [`typst::World`] implementation heavily inspired by [`typst-cli/src/world.rs`](https://github.com/typst/typst/blob/79e37ccbac080212dc42e996d760664c75d1a56f/crates/typst-cli/src/world.rs).

use std::{
    collections::{hash_map::Entry, HashMap},
    fs, io,
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use color_eyre::eyre::{eyre, Result};
use comemo::Prehashed;
use time::{OffsetDateTime, UtcOffset};
use typst::{
    diag::{eco_format, FileError, FileResult},
    foundations::{Bytes, Datetime},
    syntax::{FileId, Source, VirtualPath},
    text::{Font, FontBook, FontInfo},
    Library, World,
};

use crate::Args;

pub(crate) struct SystemWorld {
    pub(crate) library: Prehashed<Library>,
    pub(crate) book: Prehashed<FontBook>,

    root: PathBuf,
    main: FileId,
    fonts: Vec<FontSlot>,
    files: Mutex<HashMap<FileId, Bytes>>,
}

impl SystemWorld {
    pub(crate) fn new(args: &Args) -> Result<SystemWorld> {
        let mut font_db = fontdb::Database::new();
        for path in &args.font_paths {
            font_db.load_fonts_dir(path);
        }
        font_db.load_system_fonts();

        let mut book = FontBook::new();
        let mut fonts = Vec::new();
        for face in font_db.faces() {
            let info = font_db
                .with_face_data(face.id, FontInfo::new)
                .ok_or_else(|| eyre!("failed to load font file"))?;
            if let Some(info) = info {
                book.push(info);
                fonts.push(FontSlot {
                    index: face.index,
                    source: face.source.clone(),
                    font: OnceLock::new(),
                });
            }
        }

        let root = args
            .root
            .clone()
            .or_else(|| Some(args.input.canonicalize().ok()?.parent()?.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let main = {
            let input = args.input.canonicalize()?;
            let vpath = VirtualPath::within_root(&input, &root)
                .ok_or_else(|| eyre!("input file is outside root directory"))?;
            FileId::new(None, vpath)
        };

        let library = Library::builder().build();

        Ok(SystemWorld {
            library: Prehashed::new(library),
            book: Prehashed::new(book),
            root,
            main,
            fonts,
            files: Mutex::new(HashMap::new()),
        })
    }

    /// Replaces all files with a number of virtual files.
    pub(crate) fn replace_files<I>(&mut self, main: String, new_files: I)
    where
        I: IntoIterator<Item = (PathBuf, Bytes)>,
    {
        let mut files = self.files.lock().unwrap();

        self.root = PathBuf::from_iter([Component::RootDir.as_ref(), Path::new("virtual")]);
        self.main = {
            let file_id = FileId::new(None, VirtualPath::new("main.typ"));
            files.insert(file_id, Bytes::from(main.into_bytes()));
            file_id
        };

        for (path, content) in new_files {
            let file_id = FileId::new(None, VirtualPath::new(&path));
            files.insert(file_id, content);
        }
    }
}

impl World for SystemWorld {
    fn library(&self) -> &Prehashed<Library> {
        &self.library
    }

    fn book(&self) -> &Prehashed<FontBook> {
        &self.book
    }

    fn main(&self) -> Source {
        self.source(self.main).unwrap()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        let bytes = self.file(id)?;
        let string = String::from_utf8(bytes.to_vec()).map_err(|_| FileError::InvalidUtf8)?;
        Ok(Source::new(id, string))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        match self.files.lock().unwrap().entry(id) {
            Entry::Occupied(entry) => Ok(entry.get().clone()),
            Entry::Vacant(entry) => {
                let mut root = self.root.clone();
                // Get the package root. Do not download packages automatically
                // because that sounds like additional implementation work
                // and extra dependencies.
                if let Some(spec) = id.package() {
                    let package_dir: PathBuf = [
                        "typst",
                        "packages",
                        &spec.namespace,
                        &spec.name,
                        &spec.version.to_string(),
                    ]
                    .into_iter()
                    .collect();

                    root = dirs::data_dir()
                        .filter(|data_dir| data_dir.join(&package_dir).exists())
                        .or_else(dirs::cache_dir)
                        .filter(|cache_dir| cache_dir.join(&package_dir).exists())
                        .ok_or(FileError::NotFound(package_dir))?;
                }

                let path = id.vpath().resolve(&root).ok_or(FileError::AccessDenied)?;
                let bytes = fs::read(&path)
                    .map(Bytes::from)
                    .map_err(|err| match err.kind() {
                        io::ErrorKind::NotFound => FileError::NotFound(path),
                        io::ErrorKind::PermissionDenied => FileError::AccessDenied,
                        _ => FileError::Other(Some(eco_format!("{}", err))),
                    })?;
                entry.insert(bytes.clone());
                Ok(bytes)
            }
        }
    }

    fn font(&self, index: usize) -> Option<Font> {
        let slot = &self.fonts[index];
        slot.font
            .get_or_init(|| {
                let bytes = match &slot.source {
                    fontdb::Source::Binary(bytes) | fontdb::Source::SharedFile(_, bytes) => {
                        Bytes::from((**bytes).as_ref())
                    }
                    fontdb::Source::File(path) => Bytes::from(fs::read(path).ok()?),
                };
                Font::new(bytes, slot.index)
            })
            .clone()
    }

    fn today(&self, offset: Option<i64>) -> Option<Datetime> {
        let mut now = OffsetDateTime::now_utc();
        if let Some(offset) = offset {
            now = now.to_offset(UtcOffset::from_hms(offset.try_into().ok()?, 0, 0).ok()?);
        }
        Datetime::from_ymd(now.year(), now.month().into(), now.day())
    }
}

struct FontSlot {
    index: u32,
    source: fontdb::Source,
    font: OnceLock<Option<Font>>,
}
