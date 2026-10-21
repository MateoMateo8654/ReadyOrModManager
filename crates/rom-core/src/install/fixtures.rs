use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const TREE: &[(&str, &[u8])] = &[
    ("MyMod/Paks/pakchunk99-MyMod_P.pak", b"pak"),
    ("MyMod/Paks/pakchunk99-MyMod_P.utoc", b"utoc"),
    ("MyMod/Paks/pakchunk99-MyMod_P.ucas", b"ucas"),
    ("MyMod/readme.txt", b"junk"),
    (
        "__MACOSX/MyMod/Paks/._pakchunk99-MyMod_P.pak",
        b"appledouble",
    ),
];

pub fn zip(dir: &Path, name: &str, tree: &[(&str, &[u8])]) -> PathBuf {
    let p = dir.join(name);
    let mut w = zip::ZipWriter::new(fs::File::create(&p).unwrap());
    let o = zip::write::SimpleFileOptions::default();
    w.add_directory("MyMod/", o).unwrap();
    for (n, b) in tree {
        w.start_file(*n, o).unwrap();
        w.write_all(b).unwrap();
    }
    w.finish().unwrap();
    p
}

pub fn tar(dir: &Path, name: &str, tree: &[(&str, &[u8])]) -> PathBuf {
    let p = dir.join(name);
    let mut b = tar::Builder::new(fs::File::create(&p).unwrap());
    for (n, data) in tree {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, n, *data).unwrap();
    }
    b.into_inner().unwrap();
    p
}

pub fn sevenz(dir: &Path, name: &str, tree: &[(&str, &[u8])]) -> PathBuf {
    let src = dir.join(format!("{name}.src"));
    for (n, data) in tree {
        let f = src.join(n);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(f, data).unwrap();
    }
    let p = dir.join(name);
    sevenz_rust2::compress_to_path(&src, &p).unwrap();
    fs::remove_dir_all(&src).unwrap();
    p
}
