use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use flate2::read::GzDecoder;

use super::format::{read_file_header, RecordingFrame, SessionHeader};

/// Read all frames from a recording file (supports both raw and gzip-compressed).
pub fn read_recording(path: &Path) -> std::io::Result<(SessionHeader, Vec<RecordingFrame>)> {
    let file = File::open(path)?;

    if path.extension().and_then(|e| e.to_str()) == Some("gz")
        || path.to_str().unwrap_or("").ends_with(".obd2rec.gz")
    {
        read_from_reader(GzDecoder::new(BufReader::new(file)))
    } else {
        read_from_reader(BufReader::new(file))
    }
}

fn read_from_reader<R: Read>(
    mut reader: R,
) -> std::io::Result<(SessionHeader, Vec<RecordingFrame>)> {
    let (header, version) = read_file_header(&mut reader)?;
    let mut frames = Vec::new();

    while let Some(frame) = RecordingFrame::read_from(&mut reader, version)? {
        frames.push(frame);
    }

    Ok((header, frames))
}

/// Get just the header from a recording file (without reading all frames).
pub fn read_header(path: &Path) -> std::io::Result<SessionHeader> {
    let file = File::open(path)?;

    if path.to_str().unwrap_or("").ends_with(".obd2rec.gz") {
        let mut reader = GzDecoder::new(BufReader::new(file));
        let (header, _version) = read_file_header(&mut reader)?;
        Ok(header)
    } else {
        let mut reader = BufReader::new(file);
        let (header, _version) = read_file_header(&mut reader)?;
        Ok(header)
    }
}
