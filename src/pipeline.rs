use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::archiver;
use crate::fsutil;

pub const STEP_NAMES: [&str; 9] = [
    "Extract game disc image",
    "Isolate data / build RE2HD",
    "Apply 1.1.0 game EXE update",
    "Texture pack 1 - Team X HD",
    "Texture pack 2 - Seamless HD",
    "Texture pack 3 - RE-ENHANCE",
    "Classic REbirth DLLs",
    "High quality music & voice",
    "Final check",
];

#[derive(Debug, Clone)]
pub struct ModPaths {
    pub exe_patch: PathBuf,
    pub pack1: PathBuf,
    pub pack2: PathBuf,
    pub pack3: PathBuf,
    pub rebirth: PathBuf,
    pub music: PathBuf,
}

#[derive(Debug, Clone)]
pub enum Event {
    Log(String),
    Phase(usize),
    Progress(usize, f32),
    Status(String),
    Error(String),
    Prep(f32),
    Done,
}

fn send(tx: &Sender<Event>, cx: &egui::Context, ev: Event) {
    let _ = tx.send(ev);
    cx.request_repaint();
}

pub fn beep_triple() {
    // "STARRRS" in International Morse Code: dot(ding)=short, dash(DING)=long.
    // Unit = 100ms. Letters: S ... / T - / A .- / R .-. / R .-. / R .-. / S ...
    const UNIT: u64 = 100;
    let letters: [&[bool]; 7] = [
        &[false, false, false], // S ...
        &[true],                // T -
        &[false, true],         // A .-
        &[false, true, false],  // R .-.
        &[false, true, false],  // R .-.
        &[false, true, false],  // R .-.
        &[false, false, false], // S ...
    ];
    for letter in letters {
        for &is_dash in letter {
            let on_ms = UNIT * if is_dash { 3 } else { 1 };
            unsafe {
                windows_sys::Win32::System::Diagnostics::Debug::Beep(700, on_ms as u32);
            }
            std::thread::sleep(Duration::from_millis(UNIT)); // intra-letter gap
        }
        std::thread::sleep(Duration::from_millis(UNIT * 2)); // makes a 3-unit letter gap
    }
}

fn is_named(p: &Path, names: &[&str]) -> bool {
    p.file_name()
        .map(|n| {
            let n = n.to_string_lossy();
            names.iter().any(|want| n.eq_ignore_ascii_case(want))
        })
        .unwrap_or(false)
}

fn find_named(root: &Path, names: &[&str], max_depth: usize) -> Option<PathBuf> {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if is_named(&p, names) {
                    return Some(p);
                }
                if p.is_dir() && depth < max_depth {
                    stack.push((p, depth + 1));
                }
            }
        }
    }
    None
}

fn find_data_dir(stage: &Path) -> Option<PathBuf> {
    find_named(stage, &["data"], 2)
}

fn find_exe_file(root: &Path) -> Option<PathBuf> {
    let mut fallback: Option<PathBuf> = None;
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() {
                let name = p.file_name()?.to_string_lossy().to_lowercase();
                if name.ends_with(".exe") {
                    if name.starts_with("bio2") {
                        return Some(p);
                    }
                    if fallback.is_none() {
                        fallback = Some(p);
                    }
                }
            }
        }
    }
    fallback
}

fn find_subfolders(root: &Path, names: &[&str]) -> Vec<PathBuf> {
    let mut bag: Vec<PathBuf> = vec![root.to_path_buf()];
    for _ in 0..2 {
        let mut extra: Vec<PathBuf> = Vec::new();
        for dir in &bag {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    if e.path().is_dir() {
                        extra.push(e.path());
                    }
                }
            }
        }
        bag.extend(extra);
    }

    let mut found: Vec<PathBuf> = Vec::new();
    for dir in &bag {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if is_named(&p, names) && !found.contains(&p) {
                    found.push(p);
                }
            }
        }
    }

    names
        .iter()
        .filter_map(|name| found.iter().find(|p| is_named(p, &[*name])).cloned())
        .collect()
}

fn work_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("re2hd_build_{}", tag))
}

fn clear_dir(p: &Path) {
    let _ = fsutil::remove_dir_all_including_ro(p);
    let _ = std::fs::create_dir_all(p);
}

fn copy_step(
    ctx: &egui::Context,
    tx: &Sender<Event>,
    phase: usize,
    src: &Path,
    dst: &Path,
) -> Result<(), String> {
    let total = fsutil::dir_size(src).unwrap_or(0) as f32;
    let mut last_report = Instant::now();
    let mut copied = 0u64;

    fsutil::copy_tree_contents(src, dst, &mut |bytes: u64| -> bool {
        copied = bytes;
        if last_report.elapsed().as_millis() > 80 {
            let frac = if total > 0.0 {
                (bytes as f32 / total).min(1.0)
            } else {
                1.0
            };
            send(tx, ctx, Event::Progress(phase, frac));
            last_report = Instant::now();
        }
        true
    })
    .map_err(|e| format!("copy failed: {e}"))?;

    send(tx, ctx, Event::Progress(phase, 1.0));
    if total > 0.0 {
        send(
            tx,
            ctx,
            Event::Log(format!(
                "[copy] {:.1} MiB merged into RE2HD",
                copied as f32 / 1048576.0
            )),
        );
    }
    Ok(())
}

fn unpack(
    ctx: &egui::Context,
    tx: &Sender<Event>,
    seven: &Path,
    archive: &Path,
    tmp: &Path,
) -> Result<(), String> {
    send(tx, ctx, Event::Log(format!("[unpack] {}", archive.display())));
    archiver::extract(seven, archive, tmp)
}

pub fn run_pipeline(cx: egui::Context, tx_event: Sender<Event>, iso: PathBuf, mods: ModPaths) {
    let tx = tx_event;

    let fail = |msg: String| {
        send(&tx, &cx, Event::Error(msg));
    };

    let seven = match archiver::locate_7z() {
        Some(p) => p,
        None => {
            fail(
                "7-Zip not found. Place 7z.exe + 7z.dll in a 'tools' folder next to this app."
                    .to_string(),
            );
            return;
        }
    };

    send(&tx, &cx, Event::Log(format!("7-Zip engine: {}", seven.display())));

    let iso_dir = match iso.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let stem = iso
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "game".to_string());
    let stage = iso_dir.join(format!(".{}.re2hd_stage", stem));
    let re2hd = iso_dir.join("RE2HD");

    // ---- Phase 0: ISO extraction ----
    send(&tx, &cx, Event::Phase(0));
    send(&tx, &cx, Event::Status("Extracting game disc image...".into()));
    clear_dir(&stage);

    let iso_mb = std::fs::metadata(&iso).map(|m| m.len() as f32 / 1048576.0).unwrap_or(0.0);
    send(&tx, &cx, Event::Log(format!("[iso] extracting {:.0} MiB...", iso_mb)));
    if let Err(e) = unpack(&cx, &tx, &seven, &iso, &stage) {
        fail(e);
        return;
    }
    send(&tx, &cx, Event::Progress(0, 1.0));
    send(&tx, &cx, Event::Phase(1));

    // ---- Phase 1: isolate data -> RE2HD ----
    let data = match find_data_dir(&stage) {
        Some(d) => d,
        None => {
            fail("Could not locate the 'data' folder inside the extracted game disc.".into());
            return;
        }
    };

    if re2hd.exists() {
        send(&tx, &cx, Event::Log("Removing previous RE2HD output folder...".into()));
        if let Err(e) = fsutil::remove_dir_all_including_ro(&re2hd) {
            fail(format!("could not remove existing RE2HD folder: {e}"));
            return;
        }
    }

    send(&tx, &cx, Event::Status("Moving data folder as RE2HD...".into()));
    if let Err(e) = fsutil::move_into_place(&data, &re2hd) {
        fail(format!("could not finalize RE2HD folder: {e}"));
        return;
    }
    send(&tx, &cx, Event::Log("[iso] data renamed to RE2HD".into()));
    send(&tx, &cx, Event::Progress(1, 1.0));

    if stage.exists() {
        send(&tx, &cx, Event::Log("removing temporary extraction folder...".into()));
        let _ = fsutil::remove_dir_all_including_ro(&stage);
    }

    // ---- Phase 2: EXE patch ----
    send(&tx, &cx, Event::Phase(2));
    send(&tx, &cx, Event::Status("Applying 1.1.0 game EXE update...".into()));
    let tmp = work_dir("exe");
    clear_dir(&tmp);
    if let Err(e) = unpack(&cx, &tx, &seven, &mods.exe_patch, &tmp) {
        fail(e);
        return;
    }
    let patch_exe = match find_exe_file(&tmp) {
        Some(p) => p,
        None => {
            fail("No executable found inside the EXE update archive.".into());
            return;
        }
    };
    let target_exe = find_exe_file(&re2hd).unwrap_or_else(|| re2hd.join("bio2.exe"));
    let final_exe = re2hd.join("bio2.exe");
    match std::fs::copy(&patch_exe, &final_exe) {
        Ok(_) => {
            send(
                &tx,
                &cx,
                Event::Log(format!(
                    "[exe] patched {} as bio2.exe",
                    patch_exe.file_name().unwrap_or_default().to_string_lossy()
                )),
            );
            if target_exe != final_exe {
                let _ = std::fs::copy(&patch_exe, &target_exe);
            }
        }
        Err(e) => {
            fail(format!("could not write patched EXE: {e}"));
            return;
        }
    }
    let _ = fsutil::remove_dir_all_including_ro(&tmp);
    send(&tx, &cx, Event::Progress(2, 1.0));

    // ---- Phases 3..6: texture packs + rebirth DLLs -----
    let packs: [(usize, &str, &PathBuf, &str); 4] = [
        (3, "Team X Textures pack 1", &mods.pack1, "tex1"),
        (4, "Seamless HD pack 2", &mods.pack2, "tex2"),
        (5, "RE-ENHANCE pack 3", &mods.pack3, "tex3"),
        (6, "Classic REbirth DLLs", &mods.rebirth, "rebirth"),
    ];

    for (p, name, archive, tag) in packs {
        send(&tx, &cx, Event::Phase(p));
        send(&tx, &cx, Event::Status(format!("Integrating {name}...")));
        let tmp = work_dir(tag);
        clear_dir(&tmp);
        if let Err(e) = unpack(&cx, &tx, &seven, archive, &tmp) {
            fail(e);
            return;
        }
        if let Err(e) = copy_step(&cx, &tx, p, &tmp, &re2hd) {
            fail(e);
            return;
        }
        let _ = fsutil::remove_dir_all_including_ro(&tmp);
        send(&tx, &cx, Event::Log(format!("[mod] {name} applied")));
    }

    // ---- Phase 7: music & sound (COMMON / PL0 / PL1 only) ----
    send(&tx, &cx, Event::Phase(7));
    send(
        &tx,
        &cx,
        Event::Status("Integrating high quality music & voice...".into()),
    );
    let tmp = work_dir("music");
    clear_dir(&tmp);
    if let Err(e) = unpack(&cx, &tx, &seven, &mods.music, &tmp) {
        fail(e);
        return;
    }

    let wanted = ["COMMON", "PL0", "PL1"];
    let folders = find_subfolders(&tmp, &wanted);
    if folders.is_empty() {
        let _ = fsutil::remove_dir_all_including_ro(&tmp);
        fail("No COMMON / PL0 / PL1 folders found inside the music archive.".into());
        return;
    }

    for f in &folders {
        let name = f.file_name().unwrap_or_default().to_string_lossy().to_uppercase();
        send(&tx, &cx, Event::Log(format!("[music] copying {name} ...")));
        let dest = re2hd.join(&name);
        if let Err(e) = copy_step(&cx, &tx, 7, f, &dest) {
            let _ = fsutil::remove_dir_all_including_ro(&tmp);
            fail(e);
            return;
        }
        send(&tx, &cx, Event::Log(format!("[music] {name} merged")));
    }
    let _ = fsutil::remove_dir_all_including_ro(&tmp);
    send(&tx, &cx, Event::Progress(7, 1.0));

    // ---- Phase 8: final check ----
    send(&tx, &cx, Event::Phase(8));
    send(&tx, &cx, Event::Status("All archives applied. Verifying build...".into()));

    let mut found_any = false;

    if re2hd.join("bio2.exe").is_file() {
        found_any = true;
    }
    if re2hd.join("hires").is_dir() {
        found_any = true;
    }
    if re2hd.join("ddraw.dll").is_file() {
        found_any = true;
    }
    if re2hd.join("bio2hd.asi").is_file() {
        found_any = true;
    }
    if is_named(&re2hd, &["common"]) {
        found_any = true;
    }

    send(&tx, &cx, Event::Progress(8, 1.0));
    if found_any {
        send(
            &tx,
            &cx,
            Event::Status(format!("BUILD COMPLETE - RE2HD ready at {}", re2hd.display())),
        );
        send(&tx, &cx, Event::Log(format!("[done] output: {}", re2hd.display())));
    } else {
        send(
            &tx,
            &cx,
            Event::Status("Build finished but expected files were not found.".into()),
        );
        send(&tx, &cx, Event::Log("[warn] expected output files missing".into()));
    }
    send(&tx, &cx, Event::Done);

    std::thread::spawn(beep_triple);
}