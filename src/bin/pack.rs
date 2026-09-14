use std::fs::File;
use std::io::{self, Seek, Write};
use std::path::{Path, PathBuf};

const MAGIC: [u8; 8] = *b"RE2HDPLD";
const NAME_FIELD: usize = 96;

struct Entry {
    name: String,
    path: PathBuf,
}

fn entry(path: &str, name: Option<&str>) -> Entry {
    let pb = PathBuf::from(path);
    let n = match name {
        Some(n) => n.to_string(),
        None => pb
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .expect("no filename"),
    };
    Entry { name: n, path: pb }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let exe = args.next().expect("usage: pack.exe <input-exe> <output-exe> [music-dir]");
    let out_path = args.next().expect("usage: pack.exe <input-exe> <output-exe> [music-dir]");
    let music_dir = args.next();

    let mut entries: Vec<Entry> = Vec::new();
    entries.push(entry(r"tools\7z.exe", Some("7z.exe")));
    entries.push(entry(r"tools\7z.dll", Some("7z.dll")));
    entries.push(entry(
        r"E:\folder\files are here\Biohazard 2 Sourcenext 1.1.0 patch game exe update\bio2 1.10.7z",
        None,
    ));
    entries.push(entry(
        r"E:\folder\files are here\Team X Textures part 1\Resident_Evil_2_HD_mod_v20220716.zip",
        None,
    ));
    entries.push(entry(
        r"E:\folder\files are here\Resident Evil 2 - Seamless HD Project for PC Sourcenext Textures part 2\RE2_SHDP_2.0_update_for_TeamX_HD_patch.2.zip",
        None,
    ));
    entries.push(entry(
        r"E:\folder\files are here\RE-Enhance - RE2 v3.0 Textures part 3\RE-ENHANCE_RE2_v3.0.zip",
        None,
    ));
    entries.push(entry(
        r"E:\folder\files are here\Resident Evil 2 Classic REbirth DLL game root\re2cr-2024-09-01.7z",
        None,
    ));
    entries.push(entry(
        r"E:\folder\files are here\Resident Evil 2 High Quality Music & Sound ver. 2018 SourceNext\Resident Evil 2 High Quality Music & Sound ver. 2018 SourceNext.rar",
        None,
    ));

    if let Some(mdir) = music_dir {
        let mut music: Vec<PathBuf> = std::fs::read_dir(&mdir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        let l = p
                            .extension()
                            .map(|x| x.to_string_lossy().to_lowercase())
                            .unwrap_or_default();
                        l == "xm" || l == "it" || l == "mod" || l == "s3m"
                    })
                    .collect()
            })
            .unwrap_or_default();
        music.sort();
        println!("[pack] embedding {} menu music tracks from {}", music.len(), mdir);
        for m in music {
            entries.push(entry(m.to_str().unwrap(), None));
        }
    }

    for e in &entries {
        if !e.path.is_file() {
            eprintln!("[pack] MISSING payload: {}", e.path.display());
            std::process::exit(1);
        }
    }

    let mut out = File::create(&out_path).expect("cannot create output");
    let mut src = File::open(&exe).expect("cannot open input exe");
    io::copy(&mut src, &mut out).expect("cannot copy exe");

    let mut records: Vec<(String, u64, u64)> = Vec::new();
    let mut total: u64 = 0;

    println!("[pack] appending payloads -> {}", out_path);
    for e in &entries {
        let len = e.path.metadata().expect("cannot stat payload").len();
        let start = out.stream_position().expect("cannot tell");
        let mut f = File::open(&e.path).expect("cannot open payload");
        io::copy(&mut f, &mut out).expect("cannot append payload");
        records.push((e.name.clone(), start, len));
        total += len;
        println!(
            "  + {:>9} {:.1} MiB  {}",
            format_bytes(len),
            len as f32 / 1048576.0,
            e.name
        );
    }

    for (name, offset, size) in &records {
        let name_bytes = name.as_bytes();
        if name_bytes.len() > NAME_FIELD {
            eprintln!("[pack] name too long: {name}");
            std::process::exit(1);
        }
        out.write_all(&(name_bytes.len() as u32).to_le_bytes()).unwrap();
        out.write_all(&offset.to_le_bytes()).unwrap();
        out.write_all(&size.to_le_bytes()).unwrap();
        let mut pad = vec![0u8; NAME_FIELD];
        pad[..name_bytes.len()].copy_from_slice(name_bytes);
        out.write_all(&pad).unwrap();
    }
    out.write_all(&(records.len() as u32).to_le_bytes()).unwrap();
    out.write_all(&MAGIC).unwrap();
    out.flush().unwrap();

    let final_len = out.metadata().unwrap().len();
    println!(
        "[pack] done. output = {:.2} GiB (payload {:.2} GiB)",
        final_len as f32 / 1073741824.0,
        total as f32 / 1073741824.0
    );
}

fn format_bytes(n: u64) -> String {
    if n >= 1 << 30 {
        format!("{:.2} GiB", n as f32 / 1073741824.0)
    } else {
        format!("{:.1} MiB", n as f32 / 1048576.0)
    }
}

#[allow(dead_code)]
fn _uses_path(_p: &Path) {}