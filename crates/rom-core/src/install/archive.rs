use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use super::{ArchiveFormat, InstallError};
use crate::Result;

pub(super) struct Entry {
    pub name: String,
    pub size: u64,
}

pub(super) fn archive_error(archive: &Path, e: impl std::fmt::Display) -> crate::Error {
    InstallError::Archive {
        archive: archive.to_owned(),
        message: e.to_string(),
    }
    .into()
}

pub(super) fn list(archive: &Path, format: ArchiveFormat) -> Result<Vec<Entry>> {
    match format {
        ArchiveFormat::Zip => list_zip(archive),
        ArchiveFormat::SevenZ => list_7z(archive),
        ArchiveFormat::Tar => list_tar(archive),
    }
}

fn list_7z(archive: &Path) -> Result<Vec<Entry>> {
    let ar = sevenz_rust2::Archive::open(archive).map_err(|e| archive_error(archive, e))?;
    Ok(ar
        .files
        .iter()
        .filter(|e| !e.is_directory() && e.has_stream())
        .map(|e| Entry {
            name: e.name().to_owned(),
            size: e.size(),
        })
        .collect())
}

fn list_tar(archive: &Path) -> Result<Vec<Entry>> {
    let err = |e| archive_error(archive, e);
    let mut ar = tar::Archive::new(File::open(archive)?);
    let mut entries = Vec::new();
    for e in ar.entries().map_err(err)? {
        let e = e.map_err(err)?;
        if !e.header().entry_type().is_file() {
            continue;
        }
        entries.push(Entry {
            name: e.path().map_err(err)?.to_string_lossy().into_owned(),
            size: e.size(),
        });
    }
    Ok(entries)
}

pub(super) fn extract(
    archive: &Path,
    format: ArchiveFormat,
    wanted: &[(&str, &Path)],
) -> Result<()> {
    let mut pending: HashMap<&str, Vec<&Path>> = HashMap::new();
    for (entry, dest) in wanted {
        pending.entry(entry).or_default().push(dest);
    }
    match format {
        ArchiveFormat::Zip => extract_zip(archive, &mut pending)?,
        ArchiveFormat::SevenZ => extract_7z(archive, &mut pending)?,
        ArchiveFormat::Tar => extract_tar(archive, &mut pending)?,
    }
    match pending.into_keys().next() {
        None => Ok(()),
        Some(entry) => Err(InstallError::MissingEntry {
            archive: archive.to_owned(),
            entry: entry.to_owned(),
        }
        .into()),
    }
}

type Pending<'a> = HashMap<&'a str, Vec<&'a Path>>;

fn write_wanted(pending: &mut Pending, name: &str, reader: &mut dyn Read) -> io::Result<bool> {
    let Some(dests) = pending.remove(name) else {
        return Ok(false);
    };
    let Some((first, rest)) = dests.split_first() else {
        return Ok(false);
    };
    io::copy(reader, &mut File::create(first)?)?;
    for dest in rest {
        std::fs::copy(first, dest)?;
    }
    Ok(true)
}

fn extract_zip(archive: &Path, pending: &mut Pending) -> Result<()> {
    let err = |e| archive_error(archive, e);
    let mut za = zip::ZipArchive::new(File::open(archive)?).map_err(err)?;
    let names: Vec<String> = pending.keys().map(|s| (*s).to_owned()).collect();
    for name in names {
        let mut f = match za.by_name(&name) {
            Ok(f) => f,
            Err(zip::result::ZipError::FileNotFound) => continue,
            Err(e) => return Err(err(e)),
        };
        write_wanted(pending, &name, &mut f)?;
    }
    Ok(())
}

fn extract_7z(archive: &Path, pending: &mut Pending) -> Result<()> {
    let mut r = sevenz_rust2::ArchiveReader::open(archive, sevenz_rust2::Password::empty())
        .map_err(|e| archive_error(archive, e))?;
    r.for_each_entries(|e, rd| {
        if e.is_directory() || !write_wanted(pending, e.name(), rd)? {
            io::copy(rd, &mut io::sink())?;
        }
        Ok(!pending.is_empty())
    })
    .map_err(|e| archive_error(archive, e))
}

fn extract_tar(archive: &Path, pending: &mut Pending) -> Result<()> {
    let err = |e| archive_error(archive, e);
    let mut ar = tar::Archive::new(File::open(archive)?);
    for e in ar.entries().map_err(err)? {
        let mut e = e.map_err(err)?;
        if !e.header().entry_type().is_file() {
            continue;
        }
        let name = e.path().map_err(err)?.to_string_lossy().into_owned();
        write_wanted(pending, &name, &mut e)?;
        if pending.is_empty() {
            break;
        }
    }
    Ok(())
}

fn list_zip(archive: &Path) -> Result<Vec<Entry>> {
    let err = |e| archive_error(archive, e);
    let mut za = zip::ZipArchive::new(File::open(archive)?).map_err(err)?;
    let mut entries = Vec::new();
    for i in 0..za.len() {
        let f = za.by_index(i).map_err(err)?;
        if f.is_dir() {
            continue;
        }
        entries.push(Entry {
            name: f.name().map_err(err)?.into_owned(),
            size: f.size(),
        });
    }
    Ok(entries)
}
