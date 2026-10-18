//! After the download: verify and repair with par2, unpack archives with
//! 7-Zip, and clean up. Both are external programs (`par2`, `7zz`) shipped in
//! the image.

use std::path::{Path, PathBuf};

use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Par2Outcome {
    /// No par2 files came with the download.
    NoPar2,
    /// Everything was already intact.
    Verified,
    Repaired,
    /// Too much is damaged or missing.
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Tools {
    pub par2: PathBuf,
    pub sevenzip: PathBuf,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            par2: "par2".into(),
            sevenzip: "7zz".into(),
        }
    }
}

fn lower_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

async fn list_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(e) = entries.next_entry().await? {
        if e.file_type().await?.is_file() {
            out.push(e.path());
        }
    }
    out.sort();
    Ok(out)
}

/// The par2 file that describes the set: the smallest `.par2` that is not a
/// recovery volume.
pub fn main_par2(files: &[PathBuf]) -> Option<PathBuf> {
    files
        .iter()
        .filter(|f| {
            let n = lower_name(f);
            n.ends_with(".par2") && !n.contains(".vol")
        })
        .min_by_key(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(u64::MAX))
        .cloned()
}

pub async fn par2_repair(dir: &Path, tools: &Tools) -> std::io::Result<Par2Outcome> {
    let files = list_files(dir).await?;
    let Some(main) = main_par2(&files) else {
        return Ok(if files.iter().any(|f| lower_name(f).ends_with(".par2")) {
            // Only recovery volumes arrived; par2 can still use one of them.
            match files.iter().find(|f| lower_name(f).ends_with(".par2")) {
                Some(any) => run_par2(dir, tools, any).await?,
                None => Par2Outcome::NoPar2,
            }
        } else {
            Par2Outcome::NoPar2
        });
    };
    run_par2(dir, tools, &main).await
}

async fn run_par2(dir: &Path, tools: &Tools, par2_file: &Path) -> std::io::Result<Par2Outcome> {
    let output = Command::new(&tools.par2)
        .current_dir(dir)
        .args(["repair", "-q"])
        .arg(par2_file.file_name().unwrap_or_default())
        .kill_on_drop(true)
        .output()
        .await?;
    let text = String::from_utf8_lossy(&output.stdout).to_lowercase();
    if output.status.success() {
        Ok(
            if text.contains("repair complete") || text.contains("repaired") {
                Par2Outcome::Repaired
            } else {
                Par2Outcome::Verified
            },
        )
    } else {
        let tail: String = String::from_utf8_lossy(&output.stdout)
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("par2 failed")
            .to_owned();
        Ok(Par2Outcome::Failed(tail))
    }
}

fn is_first_archive_volume(name: &str) -> bool {
    // name.part01.rar / name.part1.rar are first; name.part02.rar is not.
    if let Some(stem) = name.strip_suffix(".rar") {
        if let Some((_, part)) = stem.rsplit_once(".part") {
            return part.chars().all(|c| c.is_ascii_digit()) && part.trim_start_matches('0') == "1";
        }
        return true;
    }
    name.ends_with(".7z") || name.ends_with(".zip") || name.ends_with(".001")
}

pub fn archives_to_extract(files: &[PathBuf]) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|f| is_first_archive_volume(&lower_name(f)))
        .cloned()
        .collect()
}

/// Is this a leftover piece of an archive set?
fn is_archive_part(name: &str) -> bool {
    let n = name.to_lowercase();
    if n.ends_with(".rar") || n.ends_with(".7z") || n.ends_with(".zip") {
        return true;
    }
    // .r00 .r01 ... and .001 .002 ...
    let ext = n.rsplit('.').next().unwrap_or("");
    (ext.len() == 3 && ext.starts_with('r') && ext[1..].chars().all(|c| c.is_ascii_digit()))
        || (ext.len() == 3 && ext.chars().all(|c| c.is_ascii_digit()))
}

#[derive(Debug, Default)]
pub struct ExtractReport {
    pub extracted_from: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// Unpacks every archive set in `dir` into `dir`, then removes the archives.
/// Archives that fail to extract are kept.
pub async fn extract_archives(dir: &Path, tools: &Tools) -> std::io::Result<ExtractReport> {
    let mut report = ExtractReport::default();
    // Archives can contain archives; a few rounds is plenty.
    for _ in 0..3 {
        let files = list_files(dir).await?;
        let archives = archives_to_extract(&files);
        if archives.is_empty() {
            break;
        }
        let mut progressed = false;
        for archive in archives {
            let name = lower_name(&archive);
            if report.failed.iter().any(|(n, _)| *n == name) {
                continue;
            }
            let output = Command::new(&tools.sevenzip)
                .args(["x", "-y", "-bso0", "-bsp0"])
                .arg(format!("-o{}", dir.display()))
                .arg(&archive)
                .kill_on_drop(true)
                .output()
                .await?;
            if output.status.success() {
                report.extracted_from.push(name);
                progressed = true;
                remove_archive_set(dir, &archive).await?;
            } else {
                let msg = String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .next()
                    .unwrap_or("extraction failed")
                    .to_owned();
                report.failed.push((name, msg));
            }
        }
        if !progressed {
            break;
        }
    }
    Ok(report)
}

/// Removes the volumes belonging to `first`.
async fn remove_archive_set(dir: &Path, first: &Path) -> std::io::Result<()> {
    let first_name = lower_name(first);
    let stem = first_name
        .strip_suffix(".rar")
        .map(|s| s.rsplit_once(".part").map_or(s, |(base, _)| base))
        .or_else(|| first_name.strip_suffix(".7z"))
        .or_else(|| first_name.strip_suffix(".zip"))
        .or_else(|| first_name.strip_suffix(".001"))
        .unwrap_or(&first_name)
        .to_owned();
    for f in list_files(dir).await? {
        let n = lower_name(&f);
        if n.starts_with(&stem) && is_archive_part(&n) {
            tokio::fs::remove_file(f).await?;
        }
    }
    Ok(())
}

/// Removes par2 files and other leftovers once the job is done.
pub async fn remove_par2(dir: &Path) -> std::io::Result<()> {
    for f in list_files(dir).await? {
        if lower_name(&f).ends_with(".par2") {
            tokio::fs::remove_file(f).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::Command as StdCommand;

    use super::*;

    fn have(tool: &str, arg: &str) -> bool {
        StdCommand::new(tool).arg(arg).output().is_ok()
    }

    fn names(files: &[PathBuf]) -> Vec<String> {
        files.iter().map(|f| lower_name(f)).collect()
    }

    #[test]
    fn picks_first_volumes_only() {
        let files: Vec<PathBuf> = [
            "a.part01.rar",
            "a.part02.rar",
            "a.part10.rar",
            "b.rar",
            "b.r00",
            "b.r01",
            "c.7z",
            "c.zip",
            "d.001",
            "d.002",
            "e.mkv",
            "f.part1.rar",
            "f.part2.rar",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(
            names(&archives_to_extract(&files)),
            [
                "a.part01.rar",
                "b.rar",
                "c.7z",
                "c.zip",
                "d.001",
                "f.part1.rar"
            ]
        );
        assert!(is_archive_part("b.R01") && is_archive_part("d.002") && !is_archive_part("e.mkv"));
    }

    #[tokio::test]
    async fn par2_verifies_repairs_and_fails_when_unrecoverable() {
        if !have("par2", "--help") {
            eprintln!("par2 not installed; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..200_000u32).map(|i| (i * 7 % 251) as u8).collect();
        std::fs::write(dir.path().join("movie.bin"), &data).unwrap();
        assert!(
            StdCommand::new("par2")
                .current_dir(dir.path())
                .args(["create", "-q", "-r20", "movie.par2", "movie.bin"])
                .status()
                .unwrap()
                .success()
        );

        let tools = Tools::default();
        assert_eq!(
            par2_repair(dir.path(), &tools).await.unwrap(),
            Par2Outcome::Verified
        );

        // Damage 10% of the file: repairable with 20% recovery data.
        let mut damaged = data.clone();
        damaged[1000..21000].fill(0);
        std::fs::write(dir.path().join("movie.bin"), &damaged).unwrap();
        assert_eq!(
            par2_repair(dir.path(), &tools).await.unwrap(),
            Par2Outcome::Repaired
        );
        assert_eq!(std::fs::read(dir.path().join("movie.bin")).unwrap(), data);

        // Destroy most of it: not repairable.
        std::fs::write(dir.path().join("movie.bin"), vec![0u8; 200_000]).unwrap();
        assert!(matches!(
            par2_repair(dir.path(), &tools).await.unwrap(),
            Par2Outcome::Failed(_)
        ));

        remove_par2(dir.path()).await.unwrap();
        assert_eq!(
            par2_repair(dir.path(), &tools).await.unwrap(),
            Par2Outcome::NoPar2
        );
    }

    #[tokio::test]
    async fn extracts_a_multi_volume_archive_and_removes_it() {
        if !have("7zz", "--help") {
            eprintln!("7zz not installed; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
        std::fs::write(work.path().join("movie.mkv"), &payload).unwrap();
        let made = StdCommand::new("7zz")
            .current_dir(work.path())
            .args(["a", "-v100k", "-mx0", "-bso0", "-bsp0"])
            .arg(dir.path().join("release.7z"))
            .arg("movie.mkv")
            .status()
            .unwrap();
        assert!(made.success());
        std::fs::write(dir.path().join("keep.nfo"), b"info").unwrap();
        assert!(
            std::fs::read_dir(dir.path()).unwrap().count() >= 4,
            "split into volumes"
        );

        let report = extract_archives(dir.path(), &Tools::default())
            .await
            .unwrap();
        assert!(report.failed.is_empty(), "{report:?}");
        assert_eq!(
            std::fs::read(dir.path().join("movie.mkv")).unwrap(),
            payload
        );
        let left: Vec<String> = names(&list_files(dir.path()).await.unwrap());
        assert_eq!(left, ["keep.nfo", "movie.mkv"], "volumes removed");
    }

    #[tokio::test]
    async fn a_broken_archive_is_reported_and_kept() {
        if !have("7zz", "--help") {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.zip"), b"this is not a zip file at all").unwrap();
        let report = extract_archives(dir.path(), &Tools::default())
            .await
            .unwrap();
        assert_eq!(report.failed.len(), 1);
        assert!(dir.path().join("bad.zip").exists());
    }
}
