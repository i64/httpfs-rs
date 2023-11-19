use url::Url;

use exact_reader::{ExactReader, File};
use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request,
};

use std::cmp::min;
use std::ffi::OsStr;
use std::io::{Read, Seek, SeekFrom};
use std::time::{Duration, UNIX_EPOCH};

use libc::{EINVAL, ENOENT};

use crate::adapter::RemoteAdapter;

const TTL: Duration = Duration::from_secs(1); // 1 second
const UNK_FILENAME_PREFIX: &str = "unk";

const PARENT_INO: u64 = 1;
const REMOTE_INO_START: u64 = 2;

const READ_ONLY: u16 = 0o444;

const CURRENT_DIR: FileAttr = FileAttr {
    ino: PARENT_INO,
    size: 0,
    blocks: 0,
    atime: UNIX_EPOCH,
    mtime: UNIX_EPOCH,
    ctime: UNIX_EPOCH,
    crtime: UNIX_EPOCH,
    kind: FileType::Directory,
    perm: READ_ONLY,
    nlink: 2,
    uid: 501,
    gid: 20,
    rdev: 0,
    flags: 0,
    blksize: 512,
};

const BASE_REMOTE_ATTR: FileAttr = FileAttr {
    ino: 0,
    size: 0,
    blocks: 1,
    atime: UNIX_EPOCH,
    mtime: UNIX_EPOCH,
    ctime: UNIX_EPOCH,
    crtime: UNIX_EPOCH,
    kind: FileType::RegularFile,
    perm: READ_ONLY,
    nlink: 1,
    uid: 501,
    gid: 20,
    rdev: 0,
    flags: 0,
    blksize: 512,
};

pub struct HttpFile {
    pub reader: ExactReader<File<RemoteAdapter>>,
    pub filename: String,
    pub attr: FileAttr,
}

pub struct HttpFS {
    files: Vec<HttpFile>,
}
impl HttpFS {
    pub fn try_new(client: &ureq::Agent, urls: Vec<Url>) -> Option<Self> {
        let mut unk_name_counter = 0;
        let mut ino_counter = 2;

        let mut files = Vec::with_capacity(urls.len());

        for url in urls {
            let raw_url: &str = url.as_str();
            let filename = url
                .path_segments()
                .map(|c| c.collect::<Vec<_>>())?
                .last()
                .map(|e| e.to_string())
                .or_else(|| {
                    let filename = format!("{}_{}", UNK_FILENAME_PREFIX, unk_name_counter);
                    unk_name_counter += 1;
                    Some(filename)
                })?;

            let reader = {
                let adapter = RemoteAdapter::try_new(client.clone(), raw_url)?.into();
                ExactReader::new_single(adapter)
            };

            let attr = {
                let mut attr = BASE_REMOTE_ATTR;
                attr.ino = ino_counter;
                ino_counter += 1;
                attr.size = reader.size() as u64;
                attr
            };

            files.push(HttpFile {
                reader,
                attr,
                filename,
            })
        }

        Some(Self { files })
    }

    #[inline]
    fn remote_ino_end(&self) -> u64 {
        let file_count = self.files.len() as u64;
        REMOTE_INO_START + file_count
    }

    #[inline]
    fn get_by_ino(&mut self, ino: u64) -> &mut HttpFile {
        let idx = (ino - REMOTE_INO_START) as usize;
        &mut self.files[idx]
    }
}

impl Filesystem for HttpFS {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        if parent == PARENT_INO {
            if let Some(file) = self.files.iter().find(|f| f.filename.as_str() == name) {
                return reply.entry(&TTL, &file.attr, 0);
            }
        }
        reply.error(ENOENT);
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        let remote_ino_end = self.remote_ino_end();
        if ino == PARENT_INO {
            reply.attr(&TTL, &CURRENT_DIR)
        } else if (REMOTE_INO_START..=remote_ino_end).contains(&ino) {
            reply.attr(&TTL, &self.get_by_ino(ino).attr)
        } else {
            reply.error(ENOENT)
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        let remote_ino_end = self.remote_ino_end();
        if remote_ino_end >= ino {
            let file = self.get_by_ino(ino);
            if file.reader.seek(SeekFrom::Start(offset as u64)).is_err() {
                reply.error(EINVAL);
            } else {
                let read_size = min(size, file.attr.size.saturating_sub(offset as u64) as u32);
                let mut buffer = vec![0; read_size as usize];

                let _ = file.reader.read_exact(&mut buffer);
                reply.data(&buffer)
            }
        } else {
            reply.error(ENOENT);
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        if ino != 1 {
            reply.error(ENOENT);
            return;
        }

        let entries = {
            let mut entries = vec![
                (PARENT_INO, FileType::Directory, ".".to_string()),
                (PARENT_INO, FileType::Directory, "..".to_string()),
            ];

            entries.extend(
                self.files
                    .iter()
                    .map(|file| (file.attr.ino, FileType::RegularFile, file.filename.clone())),
            );
            entries
        };

        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
            if reply.add(entry.0, (i + 1) as i64, entry.1, entry.2) {
                break;
            }
        }
        reply.ok();
    }
}
