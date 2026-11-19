//! Writes raw accelerometer samples to a CSV file, for tuning detectors on real movements.
//!
//! Format: `#` comment lines, then a `t_ns,x,y,z` header and one line per sample (sensor time in
//! nanoseconds, m/s^2). The squat tests read the same format from `tests/fixtures/squat/`.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

/// The first lines of a recording.
pub const HEADER: &str = "# mobile-sentinel accelerometer recording\n\
# To use it as a test fixture, add a line like \"# expect: 10\" (or \"# expect: 9-11\").\n\
t_ns,x,y,z\n";

/// An open recording. Dropping it flushes what is buffered.
pub struct Recorder {
    out: BufWriter<File>,
    samples: u64,
}

impl Recorder {
    /// Create (or overwrite) the file and write the header.
    pub fn create(path: &Path) -> io::Result<Self> {
        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(HEADER.as_bytes())?;
        Ok(Self { out, samples: 0 })
    }

    /// Append one sample. A write error is returned; the recording is then best abandoned.
    pub fn write(&mut self, t_ns: i64, x: f64, y: f64, z: f64) -> io::Result<()> {
        writeln!(self.out, "{t_ns},{x:.4},{y:.4},{z:.4}")?;
        self.samples += 1;
        // About once a second at game rate, so little is lost if the app is killed.
        if self.samples % 50 == 0 {
            self.out.flush()?;
        }
        Ok(())
    }

    /// Samples written so far.
    pub fn samples(&self) -> u64 {
        self.samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_header_and_one_line_per_sample() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rec.csv");
        let mut recorder = Recorder::create(&path).unwrap();
        recorder.write(1_000_000_000, 0.5, -1.25, 9.81).unwrap();
        recorder.write(1_020_000_000, 0.0, 0.0, 9.8).unwrap();
        assert_eq!(recorder.samples(), 2);
        drop(recorder);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(HEADER));
        assert!(text.ends_with(
            "t_ns,x,y,z
1000000000,0.5000,-1.2500,9.8100
1020000000,0.0000,0.0000,9.8000
"
        ));
    }
}
