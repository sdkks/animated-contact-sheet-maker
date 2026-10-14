// src/run/inputs.rs
// Turns clap positional inputs (mix of files and dirs) into the final
// per-file worklist, using input_scan for directory expansion.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

pub fn expand(inputs: &[PathBuf], recursive: bool, null: bool) -> Result<Vec<PathBuf>, String> {
    expand_from(inputs, recursive, null, &mut std::io::stdin().lock())
}

fn expand_from(inputs: &[PathBuf], recursive: bool, null: bool, reader: &mut impl BufRead) -> Result<Vec<PathBuf>, String> {
    let markers = inputs.iter().filter(|p| p.as_os_str() == "-").count();
    if markers > 1 { return Err("stdin may be used only once; remove the extra '-' input".into()); }
    if null && markers == 0 { return Err("--null requires '-' as an input".into()); }
    let mut paths = Vec::new();
    for p in inputs {
        if p.as_os_str() != "-" { paths.push(p.clone()); continue; }
        let mut reader = BufReader::new(&mut *reader);
        let mut record = Vec::new();
        let mut index = 0;
        loop {
            record.clear();
            if reader.read_until(if null { 0 } else { b'\n' }, &mut record)
                .map_err(|e| format!("cannot read stdin: {e}"))? == 0 { break; }
            index += 1;
            if record.last() == Some(&if null { 0 } else { b'\n' }) {
                record.pop();
                if !null && record.last() == Some(&b'\r') { record.pop(); }
            }
            if !null && record.contains(&0) {
                return Err(format!("stdin record {index} contains NUL; use --null"));
            }
            let path = std::str::from_utf8(&record)
                .map_err(|_| format!("stdin record {index} is not valid UTF-8"))?;
            if !path.is_empty() {
                // ffmpeg/ffprobe also reserve bare '-'; qualify this literal
                // filename so downstream tools never treat it as their stdin.
                paths.push(PathBuf::from(if path == "-" { "./-" } else { path }));
            }
        }
    }
    let mut out = Vec::new();
    for p in &paths {
        let found = acsm::input_scan::scan(Path::new(p), recursive)?;
        out.extend(found);
    }
    // Pragmatic stopgap: reject non-UTF-8 paths early with a clear error
    // rather than silently mangling them via to_string_lossy() and confusing
    // ffmpeg with a "No such file or directory" from a corrupted path string.
    // A proper fix would thread OsStr through all ffmpeg arg vectors, but that
    // is a multi-file refactor. This keeps the common case correct.
    for p in &out {
        if p.to_str().is_none() {
            return Err(format!("path is not valid UTF-8: {}", p.display()));
        }
    }
    Ok(out)
}

/// Resolve the output directory for a given source file: explicit
/// `--output` wins, otherwise emit alongside the source (fallback to `.` for bare-root paths).
pub fn out_dir(shared: &crate::cli::Shared, src: &Path) -> PathBuf {
    shared.output.clone().unwrap_or_else(|| {
        src.parent().unwrap_or(Path::new(".")).to_path_buf()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor, Read};

    struct NoRead;
    impl Read for NoRead { fn read(&mut self, _: &mut [u8]) -> io::Result<usize> { panic!("unexpected stdin read") } }
    impl BufRead for NoRead {
        fn fill_buf(&mut self) -> io::Result<&[u8]> { panic!("unexpected stdin read") }
        fn consume(&mut self, _: usize) { panic!("unexpected stdin read") }
    }

    #[test]
    fn no_marker_does_not_read_and_bad_markers_rejected_before_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("literal.mp4");
        std::fs::write(&path, b"").unwrap();
        assert_eq!(expand_from(std::slice::from_ref(&path), false, false, &mut NoRead).unwrap(), vec![path]);
        assert!(expand_from(&["-".into(), "-".into()], false, false, &mut NoRead).unwrap_err().contains("only once"));
        assert!(expand_from(&[], false, true, &mut NoRead).unwrap_err().contains("requires"));
    }

    #[test]
    fn crlf_literal_whitespace_order_and_directory_scanning() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("  literal '雪' .mp4");
        let last = dir.path().join("last.mp4");
        std::fs::write(&first, b"").unwrap();
        std::fs::write(&last, b"").unwrap();
        let data = format!("\r\n{}\r\n\n{}", first.display(), last.display());
        let got = expand_from(&[last.clone(), "-".into(), first.clone()], false, false, &mut Cursor::new(data)).unwrap();
        assert_eq!(got, vec![last.clone(), first.clone(), last, first]);
        let data = format!("{}\n", dir.path().display());
        assert_eq!(expand_from(&["-".into()], false, false, &mut Cursor::new(data)).unwrap().len(), 2);
    }

    #[test]
    fn malformed_and_empty_lists() {
        for (bytes, message) in [(vec![255], "UTF-8"), (b"a\0b".to_vec(), "use --null")] {
            assert!(expand_from(&["-".into()], false, false, &mut Cursor::new(bytes)).unwrap_err().contains(message));
        }
        assert!(expand_from(&["-".into()], false, false, &mut Cursor::new(b"\n\r\n")).unwrap().is_empty());
        assert!(expand_from(&["-".into()], false, true, &mut Cursor::new(b"\0\0")).unwrap().is_empty());
        // A record '-' is scanned as a literal path, not interpreted recursively.
        assert!(expand_from(&["-".into()], false, false, &mut Cursor::new(b"-\n")).unwrap_err().contains("path does not exist: ./-"));
    }

    #[cfg(unix)]
    #[test]
    fn nul_preserves_cr_lf_quotes_backslashes_and_unterminated_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("space \n\r'\\雪.mp4");
        std::fs::write(&path, b"").unwrap();
        let data = format!("\0{}\0{}", path.display(), path.display());
        assert_eq!(expand_from(&["-".into()], false, true, &mut Cursor::new(data)).unwrap(), vec![path.clone(), path]);
    }
}
