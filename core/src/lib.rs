//! ליבת ההרכבה והדחיסה של "מרכיב הספרייה של אוצריא".
//!
//! הפורמט זהה לזה ש-`split_release_asset.sh` של SeforimLibrary מייצר:
//! `<archive>.part-000`, `<archive>.part-001`, ... לצד `<archive>.manifest.json`
//! (schemaVersion 1) עם SHA-256 לכל חלק ולארכיון המלא.
//! הרכבה = חיבור החלקים לפי הסדר + אימות מול המניפסט.
//! דחיסה = `zstd -19` בדיוק כמו שהספרייה נבנית (חלון 8MB, סכום ביקורת פעיל).

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const COMPRESSION_LEVEL: i32 = 19;
const BUF: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Cancelled,
    Msg(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Cancelled => write!(f, "בוטל"),
            Error::Msg(m) => write!(f, "{m}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
fn msg<T>(s: impl Into<String>) -> Result<T> {
    Err(Error::Msg(s.into()))
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestPart {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema_version: u32,
    pub archive: String,
    pub size: u64,
    pub sha256: String,
    pub parts: Vec<ManifestPart>,
}

#[derive(Debug, Clone)]
pub struct SplitSet {
    /// שם הקובץ השלם אחרי ההרכבה (למשל seforim-schema7.db.zst)
    pub archive: String,
    pub parts: Vec<PathBuf>,
    pub part_sizes: Vec<u64>,
    pub total: u64,
    pub manifest: Option<Manifest>,
    pub manifest_path: Option<PathBuf>,
    /// קובץ מניפסט קיים אבל לא ניתן לקרוא או בגרסה שהכלי לא מכיר: לא מרכיבים "בלי אימות" בשקט
    pub manifest_problem: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub phase: &'static str, // "join" | "verify" | "compress"
    pub done: u64,
    pub total: u64,
    pub part_index: usize,
    pub part_count: usize,
}

#[derive(Debug, Clone)]
pub struct JoinReport {
    pub output: PathBuf,
    pub size: u64,
    pub sha256: String,
    /// true רק אם נמצא מניפסט וכל ה-SHA-256 תאמו
    pub verified: bool,
}

#[derive(Debug, Clone)]
pub struct CompressReport {
    pub output: PathBuf,
    pub input_size: u64,
    pub output_size: u64,
}

fn parse_part_name(name: &str) -> Option<(String, u32)> {
    let idx = name.rfind(".part-")?;
    let (base, num) = (&name[..idx], &name[idx + 6..]);
    if base.is_empty() || num.len() < 3 || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((base.to_string(), num.parse().ok()?))
}

/// סורק תיקייה (ללא רקורסיה) ומחזיר כל קבוצת חלקים שנמצאה.
pub fn find_sets(dir: &Path) -> Result<Vec<SplitSet>> {
    let mut groups: BTreeMap<String, BTreeMap<u32, (PathBuf, u64)>> = BTreeMap::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some((base, n)) = parse_part_name(&name) {
            let size = entry.metadata()?.len();
            groups
                .entry(base)
                .or_default()
                .insert(n, (entry.path(), size));
        }
    }
    let mut sets = Vec::new();
    for (archive, map) in groups {
        let parts: Vec<PathBuf> = map.values().map(|v| v.0.clone()).collect();
        let part_sizes: Vec<u64> = map.values().map(|v| v.1).collect();
        let manifest_file = dir.join(format!("{archive}.manifest.json"));
        let (mut manifest, mut manifest_path, mut manifest_problem) = (None, None, None);
        if let Ok(bytes) = fs::read(&manifest_file) {
            // קודם בודקים את גרסת הסכמה בלבד, כדי שגרסה עתידית תקבל הודעה מדויקת
            let ver = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|v| v.get("schemaVersion").and_then(|x| x.as_u64()));
            match ver {
                None => manifest_problem = Some("קובץ המניפסט פגום או לא קריא".to_string()),
                Some(1) => match serde_json::from_slice::<Manifest>(&bytes) {
                    Ok(m) if m.archive == archive => {
                        manifest = Some(m);
                        manifest_path = Some(manifest_file);
                    }
                    Ok(m) => {
                        manifest_problem = Some(format!(
                            "המניפסט שייך לקובץ אחר ({}) ולא ל-{archive}",
                            m.archive
                        ))
                    }
                    Err(_) => manifest_problem = Some("קובץ המניפסט פגום או לא קריא".to_string()),
                },
                Some(v) => {
                    manifest_problem = Some(format!(
                        "גרסת המניפסט ({v}) חדשה מדי ואינה מוכרת לכלי. יש להוריד גרסה עדכנית שלו"
                    ))
                }
            }
        }
        sets.push(SplitSet {
            total: part_sizes.iter().sum(),
            archive,
            parts,
            part_sizes,
            manifest,
            manifest_path,
            manifest_problem,
        });
    }
    Ok(sets)
}

/// בדיקות מקדימות מהירות: רצף חלקים, שמות וגדלים מול המניפסט.
pub fn precheck(set: &SplitSet) -> Result<()> {
    if let Some(p) = &set.manifest_problem {
        return msg(p.clone());
    }
    // רצף מספרי החלקים חייב להיות 000,001,...
    for (i, p) in set.parts.iter().enumerate() {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let (_, n) = parse_part_name(&name).unwrap();
        if n as usize != i {
            return msg(format!("חסר חלק מספר {i:03} (נמצא {name} במקומו)"));
        }
    }
    if let Some(m) = &set.manifest {
        if m.parts.len() != set.parts.len() {
            return msg(format!(
                "המניפסט מתאר {} חלקים אבל בתיקייה נמצאו {}",
                m.parts.len(),
                set.parts.len()
            ));
        }
        for (mp, (path, size)) in m.parts.iter().zip(set.parts.iter().zip(&set.part_sizes)) {
            let fname = path.file_name().unwrap().to_string_lossy();
            if mp.name != fname {
                return msg(format!("שם החלק {fname} שונה מהמניפסט ({})", mp.name));
            }
            if mp.size != *size {
                return msg(format!(
                    "גודל {fname} הוא {size} בייטים והמניפסט מצפה ל-{}. ההורדה כנראה לא הושלמה",
                    mp.size
                ));
            }
        }
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn check_free_space(dir: &Path, need: u64) -> Result<()> {
    if let Ok(free) = fs2::available_space(dir) {
        if free < need + (64 << 20) {
            return msg(format!(
                "אין מספיק מקום פנוי בתיקייה: נדרשים {} ופנויים {}",
                human(need),
                human(free)
            ));
        }
    }
    Ok(())
}

pub fn human(n: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", units[u])
    }
}

/// מוחק את הקובץ הזמני גם אם הפעולה נכשלה או בוטלה.
struct TempGuard(PathBuf, bool);
impl Drop for TempGuard {
    fn drop(&mut self) {
        if !self.1 {
            let _ = fs::remove_file(&self.0);
        }
    }
}

/// מרכיב את החלקים לקובץ אחד ב-`out_dir` ומאמת מול המניפסט.
/// הכתיבה היא לקובץ `.joining` זמני באותה תיקייה, ורק אחרי אימות מוצלח הוא נקרא בשמו הסופי.
pub fn join(
    set: &SplitSet,
    out_dir: &Path,
    overwrite: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<JoinReport> {
    precheck(set)?;
    let final_path = out_dir.join(&set.archive);
    if final_path.exists() && !overwrite {
        return msg(format!("הקובץ {} כבר קיים בתיקייה", set.archive));
    }
    check_free_space(out_dir, set.total)?;

    let tmp_path = out_dir.join(format!("{}.joining", set.archive));
    let mut guard = TempGuard(tmp_path.clone(), false);
    let mut out = BufWriter::with_capacity(BUF, File::create(&tmp_path)?);

    let mut whole = Sha256::new();
    let mut buf = vec![0u8; BUF];
    let mut done = 0u64;
    let all_ok = set.manifest.is_some();

    for (i, path) in set.parts.iter().enumerate() {
        let mut part_hash = Sha256::new();
        let mut input = BufReader::with_capacity(BUF, File::open(path)?);
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            part_hash.update(&buf[..n]);
            whole.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            done += n as u64;
            progress(Progress {
                phase: "join",
                done,
                total: set.total,
                part_index: i,
                part_count: set.parts.len(),
            });
        }
        if let Some(m) = &set.manifest {
            let got = hex(&part_hash.finalize());
            if got != m.parts[i].sha256 {
                return msg(format!(
                    "בדיקת SHA-256 נכשלה בחלק {} — הקובץ פגום, הורד אותו מחדש",
                    m.parts[i].name
                ));
            }
        }
    }
    out.flush()?;
    out.get_ref().sync_all()?;
    drop(out);

    let sha = hex(&whole.finalize());
    if let Some(m) = &set.manifest {
        if done != m.size {
            return msg(format!("גודל הקובץ המורכב {done} שונה מהמניפסט {}", m.size));
        }
        if sha != m.sha256 {
            return msg("ה-SHA-256 של הקובץ המורכב שונה מהמניפסט");
        }
    }
    if final_path.exists() {
        fs::remove_file(&final_path)?;
    }
    fs::rename(&tmp_path, &final_path)?;
    guard.1 = true;
    Ok(JoinReport {
        output: final_path,
        size: done,
        sha256: sha,
        verified: all_ok,
    })
}

/// דוחס קובץ ל-`<name>.zst` ב-`out_dir` בדיוק כמו `zstd -19`:
/// רמה 19, חלון 8MB (ברירת המחדל של הרמה), סכום ביקורת פעיל וגודל תוכן בכותרת.
pub fn compress(
    input_path: &Path,
    out_dir: &Path,
    workers: u32,
    overwrite: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<CompressReport> {
    let name = input_path
        .file_name()
        .ok_or_else(|| Error::Msg("שם קובץ לא תקין".into()))?;
    let out_name = format!("{}.zst", name.to_string_lossy());
    let final_path = out_dir.join(&out_name);
    if final_path.exists() && !overwrite {
        return msg(format!("הקובץ {out_name} כבר קיים בתיקייה"));
    }
    let in_size = fs::metadata(input_path)?.len();
    // הדחוס אף פעם לא גדול משמעותית מהמקור; דורשים מקום לפחות כגודל המקור
    check_free_space(out_dir, in_size)?;

    let tmp_path = out_dir.join(format!("{out_name}.compressing"));
    let mut guard = TempGuard(tmp_path.clone(), false);
    let file = BufWriter::with_capacity(BUF, File::create(&tmp_path)?);
    let mut enc = zstd::stream::write::Encoder::new(file, COMPRESSION_LEVEL)?;
    enc.include_checksum(true)?;
    enc.include_contentsize(true)?;
    enc.set_pledged_src_size(Some(in_size))?;
    if workers > 0 {
        enc.multithread(workers)?;
    }

    let mut input = BufReader::with_capacity(BUF, File::open(input_path)?);
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        enc.write_all(&buf[..n])?;
        done += n as u64;
        progress(Progress {
            phase: "compress",
            done,
            total: in_size,
            part_index: 0,
            part_count: 1,
        });
    }
    let mut w = enc.finish()?;
    w.flush()?;
    w.get_ref().sync_all()?;
    drop(w);
    let out_size = fs::metadata(&tmp_path)?.len();
    if final_path.exists() {
        fs::remove_file(&final_path)?;
    }
    fs::rename(&tmp_path, &final_path)?;
    guard.1 = true;
    Ok(CompressReport {
        output: final_path,
        input_size: in_size,
        output_size: out_size,
    })
}

/// מקום פנוי בבייטים בתיקייה (None אם לא ניתן לדעת).
pub fn free_space(dir: &Path) -> Option<u64> {
    fs2::available_space(dir).ok()
}

/// גודל ה-DB אחרי הפענוח, כפי שנכתב בכותרת קובץ ה-zstd (None אם לא מופיע).
pub fn content_size(set: &SplitSet) -> Option<u64> {
    let mut f = File::open(set.parts.first()?).ok()?;
    let mut head = [0u8; 32];
    let n = f.read(&mut head).ok()?;
    zstd::zstd_safe::get_frame_content_size(&head[..n])
        .ok()
        .flatten()
}

/// שם קובץ ה-DB שייווצר בחילוץ (שם הארכיון בלי `.zst`).
pub fn extracted_name(archive: &str) -> Option<String> {
    archive
        .strip_suffix(".zst")
        .filter(|n| !n.is_empty())
        .map(|n| n.to_string())
}

/// קורא את החלקים ברצף כזרם אחד, ומחשב SHA-256 לכל חלק ולכלל הארכיון תוך כדי הקריאה.
struct PartsReader<'a> {
    set: &'a SplitSet,
    idx: usize,
    cur: Option<BufReader<File>>,
    part_hash: Sha256,
    whole: Sha256,
    done: u64,
    cancel: &'a AtomicBool,
    progress: &'a mut dyn FnMut(Progress),
    problem: Option<String>,
}

impl<'a> PartsReader<'a> {
    fn fail<T>(&mut self, m: String) -> io::Result<T> {
        self.problem = Some(m.clone());
        Err(io::Error::other(m))
    }
}

impl<'a> Read for PartsReader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(io::Error::other("cancelled"));
            }
            if self.cur.is_none() {
                if self.idx >= self.set.parts.len() {
                    return Ok(0);
                }
                self.cur = Some(BufReader::with_capacity(
                    BUF,
                    File::open(&self.set.parts[self.idx])?,
                ));
                self.part_hash = Sha256::new();
            }
            let n = self.cur.as_mut().unwrap().read(buf)?;
            if n > 0 {
                self.part_hash.update(&buf[..n]);
                self.whole.update(&buf[..n]);
                self.done += n as u64;
                (self.progress)(Progress {
                    phase: "extract",
                    done: self.done,
                    total: self.set.total,
                    part_index: self.idx,
                    part_count: self.set.parts.len(),
                });
                return Ok(n);
            }
            // הגענו לסוף החלק: מאמתים אותו מול המניפסט לפני שממשיכים
            if let Some(m) = &self.set.manifest {
                let got = hex(&std::mem::take(&mut self.part_hash).finalize());
                if got != m.parts[self.idx].sha256 {
                    let name = m.parts[self.idx].name.clone();
                    return self.fail(format!(
                        "בדיקת SHA-256 נכשלה בחלק {name} — הקובץ פגום, הורד אותו מחדש"
                    ));
                }
            }
            self.cur = None;
            self.idx += 1;
        }
    }
}

/// מחבר את החלקים ופותח אותם ישר לקובץ ה-DB, בלי קובץ `.zst` ביניים.
/// מאומתים: SHA-256 של כל חלק ושל הארכיון (מול המניפסט), וסכום הביקורת הפנימי של zstd.
/// הכתיבה לקובץ זמני `.extracting`; רק אחרי הצלחה מלאה הוא מקבל את שמו הסופי.
pub fn extract(
    set: &SplitSet,
    out_dir: &Path,
    overwrite: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<JoinReport> {
    precheck(set)?;
    let name = extracted_name(&set.archive)
        .ok_or_else(|| Error::Msg("חילוץ נתמך רק לקבצי .zst".into()))?;
    let final_path = out_dir.join(&name);
    if final_path.exists() && !overwrite {
        return msg(format!("הקובץ {name} כבר קיים בתיקייה"));
    }
    // מקום נדרש: גודל ה-DB מהכותרת, ואם אינו ידוע, הערכה שמרנית
    check_free_space(
        out_dir,
        content_size(set).unwrap_or(set.total.saturating_mul(4)),
    )?;

    let tmp_path = out_dir.join(format!("{name}.extracting"));
    let mut guard = TempGuard(tmp_path.clone(), false);
    let mut out = BufWriter::with_capacity(BUF, File::create(&tmp_path)?);

    let mut rd = PartsReader {
        set,
        idx: 0,
        cur: None,
        part_hash: Sha256::new(),
        whole: Sha256::new(),
        done: 0,
        cancel,
        progress,
        problem: None,
    };
    let mut produced = 0u64;
    let res: io::Result<()> = (|| {
        let mut dec = zstd::stream::read::Decoder::new(&mut rd)?;
        let mut buf = vec![0u8; 4 << 20];
        loop {
            let n = dec.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            produced += n as u64;
        }
        Ok(())
    })();
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    if let Some(p) = rd.problem.take() {
        return msg(p);
    }
    if let Err(e) = res {
        return msg(format!(
            "הפענוח נכשל: {e}. ייתכן שהקבצים פגומים או שאינם בפורמט zstd"
        ));
    }
    // מוודאים שכל הבייטים נקראו ושכל החלקים אומתו (למקרה שיש נתונים אחרי סוף הזרם)
    io::copy(&mut rd, &mut io::sink())
        .map_err(|e| Error::Msg(rd.problem.clone().unwrap_or(e.to_string())))?;
    if let Some(p) = rd.problem.take() {
        return msg(p);
    }
    out.flush()?;
    out.get_ref().sync_all()?;
    drop(out);

    let sha = hex(&std::mem::replace(&mut rd.whole, Sha256::new()).finalize());
    if let Some(m) = &set.manifest {
        if rd.done != m.size {
            return msg(format!(
                "גודל הארכיון שנקרא {} שונה מהמניפסט {}",
                rd.done, m.size
            ));
        }
        if sha != m.sha256 {
            return msg("ה-SHA-256 של הארכיון שונה מהמניפסט");
        }
    }
    if let Some(expect) = content_size(set) {
        if produced != expect {
            return msg(format!(
                "גודל ה-DB שחולץ ({produced}) שונה מהמצופה ({expect})"
            ));
        }
    }
    if final_path.exists() {
        fs::remove_file(&final_path)?;
    }
    fs::rename(&tmp_path, &final_path)?;
    guard.1 = true;
    Ok(JoinReport {
        output: final_path,
        size: produced,
        sha256: sha,
        verified: set.manifest.is_some(),
    })
}
