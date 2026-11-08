//! Loads a real VST3/CLAP bundle and reports what the host actually sees: the
//! parameter table and the RMS of a rendered block, so hosting can be verified
//! without the GUI.
//!
//! `cargo run -p reafl-hosting --example probe -- <path> [instrument]`

use reafl_hosting::load;

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: probe <bundle> [instrument]");
        std::process::exit(2);
    };
    let instrument = args.next().as_deref() == Some("instrument");
    let (ui, mut audio) = match load(&path, instrument, 48_000.0, 64) {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("load failed: {error}");
            std::process::exit(1);
        }
    };
    let info = ui.info().clone();
    println!("name       : {}", info.name);
    println!("vendor     : {}", info.vendor);
    println!("version    : {}", info.version);
    println!("editor     : {}", info.supports_editor);
    println!("midi input : {}", info.accepts_midi);
    let parameters = ui.parameters();
    println!("parameters : {}", parameters.len());
    for parameter in parameters.iter().take(8) {
        println!(
            "  [{:>4}] {:<28} {:.3}",
            parameter.id, parameter.name, parameter.value
        );
    }

    if instrument {
        audio.send_note(60, 0.8, true);
    }
    let mut left = vec![0.0f32; 64];
    let mut right = vec![0.0f32; 64];
    let mut peak = 0.0f32;
    for block in 0..64 {
        if instrument {
            left.fill(0.0);
            right.fill(0.0);
        } else if block == 0 {
            // One block of white noise is enough to excite most effects.
            for (index, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
                let noise = ((index as f32 * 12.9898).sin() * 43758.547).fract() - 0.5;
                *l = noise * 0.5;
                *r = noise * 0.5;
            }
        } else {
            left.fill(0.0);
            right.fill(0.0);
        }
        audio.process(&mut left, &mut right);
        peak = peak.max(rms(&left).max(rms(&right)));
    }
    println!("rendered rms: {peak:.5}");
    if instrument && peak <= 1e-6 {
        eprintln!("warning: instrument produced silence");
    }
    drop(audio);
    drop(ui);
}
