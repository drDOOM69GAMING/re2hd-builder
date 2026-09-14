use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use re2hd_builder::{embedded, pipeline};
use re2hd_builder::pipeline::{Event, ModPaths};

fn main() {
    let iso = PathBuf::from(r"E:\folder\files are here\biohazard-2-apan-source-next.iso");

    let (mods, source_note) = match embedded::scan().expect("embedded scan") {
        Some(payloads) => {
            let totals: u64 = payloads.iter().map(|p| p.size).sum();
            if !embedded::all_cached(&payloads) {
                println!("[headless] extracting {:.2} GiB of embedded payloads to temp...", totals as f32 / 1073741824.0);
                embedded::extract_all(&payloads, &mut |done, total| {
                    println!("  prep {:.0}%", done as f32 / total as f32 * 100.0);
                })
                .expect("embedded extraction");
            }
            let bundle = embedded::resolve(&payloads);
            match bundle.mods {
                Some(m) => (m, "embedded payloads".to_string()),
                None => {
                    eprintln!("[headless] embedded payloads present but incomplete");
                    std::process::exit(1);
                }
            }
        }
        None => {
            let m = ModPaths {
                exe_patch: PathBuf::from(
                    r"E:\folder\files are here\Biohazard 2 Sourcenext 1.1.0 patch game exe update\bio2 1.10.7z",
                ),
                pack1: PathBuf::from(
                    r"E:\folder\files are here\Team X Textures part 1\Resident_Evil_2_HD_mod_v20220716.zip",
                ),
                pack2: PathBuf::from(
                    r"E:\folder\files are here\Resident Evil 2 - Seamless HD Project for PC Sourcenext Textures part 2\RE2_SHDP_2.0_update_for_TeamX_HD_patch.2.zip",
                ),
                pack3: PathBuf::from(
                    r"E:\folder\files are here\RE-Enhance - RE2 v3.0 Textures part 3\RE-ENHANCE_RE2_v3.0.zip",
                ),
                rebirth: PathBuf::from(
                    r"E:\folder\files are here\Resident Evil 2 Classic REbirth DLL game root\re2cr-2024-09-01.7z",
                ),
                music: PathBuf::from(
                    r"E:\folder\files are here\Resident Evil 2 High Quality Music & Sound ver. 2018 SourceNext\Resident Evil 2 High Quality Music & Sound ver. 2018 SourceNext.rar",
                ),
            };
            (m, "direct file paths".to_string())
        }
    };

    println!("[headless] mode: {source_note}");
    println!("[headless] verifying inputs...");
    for (name, p) in [
        ("ISO", &iso),
        ("EXE update", &mods.exe_patch),
        ("Team X pack", &mods.pack1),
        ("SHDP pack", &mods.pack2),
        ("RE-ENHANCE", &mods.pack3),
        ("REbirth DLLs", &mods.rebirth),
        ("Music & sound", &mods.music),
    ] {
        if p.is_file() {
            println!("  OK   {name}: {}", p.display());
        } else {
            println!("  MISS {name}: {}", p.display());
        }
    }

    let ctx = eframe::egui::Context::default();
    let (tx, rx) = mpsc::channel();

    let mut last_phase_time = Instant::now();
    let mut current_phase = usize::MAX;
    let total_started = Instant::now();

    let handle = std::thread::spawn(move || {
        pipeline::run_pipeline(ctx, tx, iso, mods);
    });

    while let Ok(ev) = rx.recv() {
        match ev {
            Event::Log(l) => println!("  [log] {l}"),
            Event::Phase(p) => {
                if current_phase != usize::MAX {
                    println!(
                        "  phase {} done in {:.1}s",
                        current_phase,
                        last_phase_time.elapsed().as_secs_f32()
                    );
                }
                current_phase = p;
                last_phase_time = Instant::now();
                println!(
                    "=== PHASE {p}: {} ===",
                    pipeline::STEP_NAMES.get(p).copied().unwrap_or("?")
                );
            }
            Event::Progress(p, f) => {
                if p != current_phase {
                    println!("  phase {p} progress {:.0}%", f * 100.0);
                } else if f > 0.01 {
                    println!("  ... {:.0}%", f * 100.0);
                }
            }
            Event::Prep(f) => println!("  [prep] {:.0}%", f * 100.0),
            Event::Status(s) => println!("  [status] {s}"),
            Event::Error(e) => {
                println!("  [ERROR] {e}");
                break;
            }
            Event::Done => {
                println!("=== DONE in {:.1}s ===", total_started.elapsed().as_secs_f32());
                break;
            }
        }
    }

    let _ = handle.join();
}