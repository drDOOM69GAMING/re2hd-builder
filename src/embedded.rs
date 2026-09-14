use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::pipeline::ModPaths;

const MAGIC: [u8; 8] = *b"RE2HDPLD";
const NAME_FIELD: usize = 96;
const RECORD_SIZE: usize = 4 + 8 + 8 + NAME_FIELD; // len, offset, size, name
const CELL_SIZE: usize = 4 + 8 + 8; // len + offset + size

#[derive(Debug, Clone)]
pub struct Payload {
    pub name: String,
    pub offset: u64,
    pub size: u64,
}

pub fn embedded_root() -> PathBuf {
    std::env::temp_dir().join("re2hd_embedded")
}

fn read_exact_at<R: Read + Seek + ?Sized>(r: &mut R, pos: u64, buf: &mut [u8]) -> io::Result<()> {
    r.seek(SeekFrom::Start(pos))?;
    r.read_exact(buf)
}

/// Scan the running exe for an appended payload index. Returns None when not packed.
pub fn scan() -> Result<Option<Vec<Payload>>, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot locate this executable: {e}"))?;
    let mut f = File::open(&exe).map_err(|e| format!("cannot open self: {e}"))?;

    let len = f.metadata().map_err(|e| format!("cannot stat self: {e}"))?.len();
    if len < (12 + RECORD_SIZE as u64) {
        return Ok(None);
    }

    let mut tail = [0u8; 12];
    read_exact_at(&mut f, len - 12, &mut tail).map_err(|e| format!("index read: {e}"))?;

    // tail layout: [count u32][magic 8 bytes]
    let count = u32::from_le_bytes(tail[0..4].try_into().unwrap()) as usize;
    if tail[4..12] != MAGIC {
        return Ok(None);
    }
    if count == 0 || count > 4096 {
        return Err("corrupt embedded index (bad count).".to_string());
    }

    let records_start = len - 12 - (count * RECORD_SIZE) as u64;
    let mut payloads = Vec::with_capacity(count);
    let mut pos = records_start;
    for _ in 0..count {
        let mut cell = [0u8; CELL_SIZE];
        read_exact_at(&mut f, pos, &mut cell).map_err(|e| format!("index read: {e}"))?;
        let name_len = u32::from_le_bytes(cell[0..4].try_into().unwrap()) as usize;
        let offset = u64::from_le_bytes(cell[4..12].try_into().unwrap());
        let size = u64::from_le_bytes(cell[12..20].try_into().unwrap());

        let mut name_buf = vec![0u8; NAME_FIELD];
        read_exact_at(&mut f, pos + CELL_SIZE as u64, &mut name_buf)
            .map_err(|e| format!("index read: {e}"))?;
        pos += RECORD_SIZE as u64;
        if name_len > NAME_FIELD {
            return Err("corrupt embedded index (name too long).".to_string());
        }
        let name = String::from_utf8(name_buf[..name_len].to_vec())
            .map_err(|_| "corrupt embedded index (bad name).".to_string())?;
        payloads.push(Payload { name, offset, size });
    }
    Ok(Some(payloads))
}

/// Extract a single payload region from the current exe into `dest`.
pub fn extract_payload(payload: &Payload, dest: &Path) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let mut f = File::open(exe)?;
    f.seek(SeekFrom::Start(payload.offset))?;
    let mut limited = f.take(payload.size);
    let mut out = File::create(dest)?;
    io::copy(&mut limited, &mut out)?;
    Ok(())
}

pub fn all_cached(payloads: &[Payload]) -> bool {
    let root = embedded_root();
    payloads
        .iter()
        .all(|p| root.join(&p.name).metadata().map(|m| m.len() == p.size).unwrap_or(false))
}

/// Extract all payloads into TEMP/re2hd_embedded, reporting fraction done.
pub fn extract_all(
    payloads: &[Payload],
    on_progress: &mut dyn FnMut(u64, u64), // (bytes_done, bytes_total)
) -> io::Result<()> {
    let root = embedded_root();
    std::fs::create_dir_all(&root)?;

    let byte_total: u64 = payloads.iter().map(|p| p.size).sum();
    let mut done: u64 = 0;

    for p in payloads {
        let dest = root.join(&p.name);
        if dest.metadata().map(|m| m.len() == p.size).unwrap_or(false) {
            done += p.size;
            on_progress(done, byte_total);
            continue;
        }
        extract_payload(p, &dest)?;
        done += p.size;
        on_progress(done, byte_total);
    }
    Ok(())
}

fn slot_for(name: &str) -> Option<usize> {
    let lower = name.to_lowercase();
    if lower.ends_with(".exe") {
        return Some(0); // 7z.exe
    }
    if lower.ends_with(".dll") {
        return Some(1); // 7z.dll
    }
    if lower.ends_with(".xm") || lower.ends_with(".it") || lower.ends_with(".mod") || lower.ends_with(".s3m")
    {
        return Some(8); // menu music
    }
    let heuristics: [(usize, &[&str]); 6] = [
        (2, &["1.10"]),     // exe patch
        (3, &["hd_mod"]),   // team x
        (4, &["shdp"]),     // seamless
        (5, &["enhance"]),  // re-enhance
        (6, &["re2cr"]),    // rebirth
        (7, &["music"]),    // music
    ];
    heuristics
        .iter()
        .find(|(_, needles)| needles.iter().all(|n| lower.contains(n)))
        .map(|(slot, _)| *slot)
}

pub struct Bundle {
    pub seven: PathBuf,
    pub seven_dll: PathBuf,
    pub mods: Option<ModPaths>,
    pub music: Vec<PathBuf>,
}

/// Map embedded payloads to a ready-to-use bundle (7z engine + mod paths + music tracks).
pub fn resolve(payloads: &[Payload]) -> Bundle {
    let root = embedded_root();
    let mut seven = PathBuf::new();
    let mut seven_dll = PathBuf::new();
    let mut music: Vec<PathBuf> = Vec::new();
    let mut mods = ModPaths {
        exe_patch: PathBuf::new(),
        pack1: PathBuf::new(),
        pack2: PathBuf::new(),
        pack3: PathBuf::new(),
        rebirth: PathBuf::new(),
        music: PathBuf::new(),
    };
    let mut found: Vec<bool> = vec![false; 6];

    for p in payloads {
        let dest = root.join(&p.name);
        match slot_for(&p.name) {
            Some(0) => seven = dest,
            Some(1) => seven_dll = dest,
            Some(2) => {
                mods.exe_patch = dest;
                found[0] = true;
            }
            Some(3) => {
                mods.pack1 = dest;
                found[1] = true;
            }
            Some(4) => {
                mods.pack2 = dest;
                found[2] = true;
            }
            Some(5) => {
                mods.pack3 = dest;
                found[3] = true;
            }
            Some(6) => {
                mods.rebirth = dest;
                found[4] = true;
            }
            Some(7) => {
                mods.music = dest;
                found[5] = true;
            }
            Some(8) => music.push(dest),
            _ => {}
        }
    }

    music.sort();
    let complete = found.iter().all(|b| *b) && !seven.as_os_str().is_empty();
    Bundle {
        seven,
        seven_dll,
        mods: if complete { Some(mods) } else { None },
        music,
    }
}