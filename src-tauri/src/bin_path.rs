use std::path::Path;

/// Resolves an external CLI tool (ffmpeg/ffprobe/cd-paranoia) to an absolute
/// path when possible. A packaged .app launched from Finder/LaunchServices
/// gets a minimal PATH that doesn't include Homebrew's bin dirs, so a bare
/// `Command::new("ffmpeg")` that works under `cargo tauri dev` (inherits the
/// terminal's PATH) fails with "No such file or directory" once bundled.
/// Falls back to the bare name so it still resolves via PATH in dev/CI.
pub fn resolve(name: &str) -> String {
    const COMMON_DIRS: &[&str] = &[
        "/opt/homebrew/bin", // Homebrew, Apple Silicon
        "/usr/local/bin",    // Homebrew, Intel
        "/usr/bin",
        "/bin",
    ];

    for dir in COMMON_DIRS {
        let candidate = Path::new(dir).join(name);
        if candidate.is_file() {
            return candidate.to_string_lossy().into_owned();
        }
    }

    name.to_string()
}
