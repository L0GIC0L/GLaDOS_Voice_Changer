//! Text to speech through installed command-line engines: eSpeak NG (robotic, always a good GLaDOS base) and, if
//! present, Piper (natural neural voices; needs a downloaded .onnx voice model).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use glados_dsp::{Audio, Wav};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Espeak,
    Piper,
}

#[derive(Clone, Debug)]
pub struct Voice {
    /// What eSpeak NG's -v takes, such as "en-us".
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub text: String,
    pub engine: Engine,
    /// eSpeak NG voice id.
    pub voice: String,
    /// eSpeak NG only: use the "f3" female variant.
    pub female: bool,
    /// Piper only: path to the .onnx model.
    pub piper_model: String,
    /// 0.5 to 2, 1 is normal.
    pub rate: f32,
    /// eSpeak NG only: 0.5 to 2, 1 is normal.
    pub pitch: f32,
}

const TIMEOUT: Duration = Duration::from_secs(90);

pub fn find_program(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

pub fn espeak_available() -> bool {
    find_program("espeak-ng").is_some()
}

pub fn piper_available() -> bool {
    find_program("piper").is_some()
}

/// eSpeak NG's voices, English first.
pub fn espeak_voices() -> Vec<Voice> {
    let Ok(out) = Command::new("espeak-ng").arg("--voices").output() else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut voices: Vec<Voice> = text
        .lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            (cols.len() >= 4).then(|| Voice { id: cols[1].to_string(), label: format!("{} ({})", cols[3].replace('_', " "), cols[1]) })
        })
        .collect();
    voices.sort_by_key(|v| (!v.id.starts_with("en"), v.label.clone()));
    voices
}

fn temp_wav() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!("glados-voice-{}-{}.wav", std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
}

/// Speaks `req.text` into audio. Blocks, so call it off the UI thread.
pub fn synthesize(req: &Request) -> Result<Audio, String> {
    let out = temp_wav();
    let result = run(req, &out).and_then(|_| {
        let bytes = std::fs::read(&out).map_err(|e| format!("The speech engine wrote no audio ({e})"))?;
        Wav::read(&bytes)
    });
    let _ = std::fs::remove_file(&out);
    let audio = result?;
    if audio.samples.iter().all(|v| *v == 0.0) {
        return Err("The speech engine produced silence.".into());
    }
    Ok(audio)
}

fn run(req: &Request, out: &Path) -> Result<(), String> {
    let mut cmd;
    match req.engine {
        Engine::Espeak => {
            cmd = Command::new("espeak-ng");
            let voice = if req.female { format!("{}+f3", req.voice) } else { req.voice.clone() };
            cmd.arg("-v").arg(voice)
                .arg("-s").arg(((175.0 * req.rate).round() as i32).clamp(80, 450).to_string())
                .arg("-p").arg(((50.0 * req.pitch).round() as i32).clamp(0, 99).to_string())
                .arg("-w").arg(out)
                .arg("--stdin");
        }
        Engine::Piper => {
            if req.piper_model.is_empty() || !Path::new(&req.piper_model).is_file() {
                return Err("Choose a Piper voice model (.onnx file) first.".into());
            }
            cmd = Command::new("piper");
            cmd.arg("--model").arg(&req.piper_model)
                .arg("--output_file").arg(out)
                .arg("--length_scale").arg(format!("{:.3}", 1.0 / req.rate.clamp(0.25, 4.0)));
        }
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start the speech engine: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(req.text.as_bytes()).map_err(|e| e.to_string())?;
    }
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut e, &mut err);
                }
                return Err(format!("The speech engine failed ({status}). {}", err.trim()));
            }
            Ok(None) if start.elapsed() > TIMEOUT => {
                let _ = child.kill();
                return Err("The speech engine did not answer in time.".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    }
}
