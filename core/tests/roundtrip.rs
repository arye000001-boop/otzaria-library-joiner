use joiner_core::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("joiner-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}
fn sha(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}
/// יוצר חלקים ומניפסט באותה צורה ש-split_release_asset.sh מייצר
fn make_split(dir: &PathBuf, name: &str, data: &[u8], part: usize) {
    let mut parts = vec![];
    for (i, c) in data.chunks(part).enumerate() {
        let n = format!("{name}.part-{i:03}");
        fs::write(dir.join(&n), c).unwrap();
        parts.push(format!(r#"{{"name":"{n}","size":{},"sha256":"{}"}}"#, c.len(), sha(c)));
    }
    let m = format!(
        r#"{{"schemaVersion":1,"archive":"{name}","size":{},"sha256":"{}","partSizeLimit":{part},"githubAssetLimit":2147483648,"parts":[{}]}}"#,
        data.len(), sha(data), parts.join(",")
    );
    fs::write(dir.join(format!("{name}.manifest.json")), m).unwrap();
}
fn data(n: usize) -> Vec<u8> {
    let mut x = 12345u64;
    (0..n).map(|_| { x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); (x >> 56) as u8 }).collect()
}

#[test]
fn join_matches_original_and_verifies() {
    let d = tmpdir("join");
    let orig = data(3_500_000);
    make_split(&d, "lib.db.zst", &orig, 1_000_000);
    let sets = find_sets(&d).unwrap();
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].parts.len(), 4);
    let out = tmpdir("joinout");
    let c = AtomicBool::new(false);
    let r = join(&sets[0], &out, false, &c, &mut |_| {}).unwrap();
    assert!(r.verified);
    assert_eq!(fs::read(&r.output).unwrap(), orig);
    // אין שאריות
    assert!(!out.join("lib.db.zst.joining").exists());
}

#[test]
fn corrupt_part_is_rejected_and_leaves_nothing() {
    let d = tmpdir("bad");
    let orig = data(2_000_000);
    make_split(&d, "lib.db.zst", &orig, 1_000_000);
    let p = d.join("lib.db.zst.part-001");
    let mut b = fs::read(&p).unwrap();
    b[10] ^= 0xFF;
    fs::write(&p, b).unwrap();
    let sets = find_sets(&d).unwrap();
    let out = tmpdir("badout");
    let r = join(&sets[0], &out, false, &AtomicBool::new(false), &mut |_| {});
    assert!(r.is_err());
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
}

#[test]
fn truncated_part_and_missing_part_are_rejected() {
    let d = tmpdir("trunc");
    make_split(&d, "lib.db.zst", &data(2_500_000), 1_000_000);
    let p = d.join("lib.db.zst.part-002");
    let b = fs::read(&p).unwrap();
    fs::write(&p, &b[..b.len() - 5]).unwrap();
    let sets = find_sets(&d).unwrap();
    assert!(precheck(&sets[0]).is_err());
    fs::remove_file(d.join("lib.db.zst.part-001")).unwrap();
    let sets = find_sets(&d).unwrap();
    assert!(precheck(&sets[0]).is_err());
}

#[test]
fn works_without_manifest_but_not_verified() {
    let d = tmpdir("nom");
    make_split(&d, "lib.db.zst", &data(1_500_000), 1_000_000);
    fs::remove_file(d.join("lib.db.zst.manifest.json")).unwrap();
    let sets = find_sets(&d).unwrap();
    let out = tmpdir("nomout");
    let r = join(&sets[0], &out, false, &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert!(!r.verified);
}

#[test]
fn refuses_to_overwrite_unless_asked() {
    let d = tmpdir("ow");
    make_split(&d, "lib.db.zst", &data(1_200_000), 1_000_000);
    let sets = find_sets(&d).unwrap();
    let out = tmpdir("owout");
    fs::write(out.join("lib.db.zst"), b"x").unwrap();
    assert!(join(&sets[0], &out, false, &AtomicBool::new(false), &mut |_| {}).is_err());
    assert!(join(&sets[0], &out, true, &AtomicBool::new(false), &mut |_| {}).is_ok());
}

#[test]
fn compress_roundtrip_and_frame_parameters() {
    let d = tmpdir("cmp");
    let mut text = Vec::new();
    for i in 0..200_000 { text.extend_from_slice(format!("שורה {i} של טקסט עברי לדוגמה\n").as_bytes()); }
    fs::write(d.join("x.db"), &text).unwrap();
    let out = tmpdir("cmpout");
    let r = compress(&d.join("x.db"), &out, 2, false, &AtomicBool::new(false), &mut |_| {}).unwrap();
    let z = fs::read(&r.output).unwrap();
    assert_eq!(&z[..4], &[0x28, 0xb5, 0x2f, 0xfd]);
    assert_ne!(z[4] & 0x04, 0, "סכום ביקורת חייב להיות פעיל");
    assert_eq!(zstd::decode_all(&z[..]).unwrap(), text);
}

#[test]
fn unknown_manifest_version_is_blocked_not_silently_unverified() {
    let d = tmpdir("ver");
    make_split(&d, "lib.db.zst", &data(1_500_000), 1_000_000);
    let m = d.join("lib.db.zst.manifest.json");
    let txt = fs::read_to_string(&m).unwrap().replace("\"schemaVersion\":1", "\"schemaVersion\":2");
    fs::write(&m, txt).unwrap();
    let sets = find_sets(&d).unwrap();
    let e = precheck(&sets[0]).unwrap_err().to_string();
    assert!(e.contains("2"), "{e}");
    fs::write(&m, "not json").unwrap();
    assert!(precheck(&find_sets(&d).unwrap()[0]).is_err());
}

#[test]
fn many_parts_in_order_including_three_and_four() {
    for n in [3usize, 4, 12] {
        let d = tmpdir(&format!("n{n}"));
        let orig = data(n * 300_000 - 17);
        make_split(&d, "lib.db.zst", &orig, 300_000);
        let sets = find_sets(&d).unwrap();
        assert_eq!(sets[0].parts.len(), n);
        let out = tmpdir(&format!("n{n}out"));
        let r = join(&sets[0], &out, false, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert!(r.verified);
        assert_eq!(fs::read(&r.output).unwrap(), orig);
    }
}
