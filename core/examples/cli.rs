// כלי בדיקה בשורת פקודה: cli join <dir> <outdir> | cli compress <file> <outdir> <workers>
use joiner_core::*;
use std::path::Path;
use std::sync::atomic::AtomicBool;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let cancel = AtomicBool::new(false);
    let mut p = |_p: Progress| {};
    match a[1].as_str() {
        "join" => {
            let sets = find_sets(Path::new(&a[2])).unwrap();
            for s in sets {
                let r = join(&s, Path::new(&a[3]), true, &cancel, &mut p).unwrap();
                println!("{:?}", r);
            }
        }
        "extract" => {
            let sets = find_sets(Path::new(&a[2])).unwrap();
            for s in sets {
                let r = extract(&s, Path::new(&a[3]), true, &cancel, &mut p).unwrap();
                println!("{:?}", r);
            }
        }
        "compress" => {
            let r = compress(
                Path::new(&a[2]),
                Path::new(&a[3]),
                a[4].parse().unwrap(),
                true,
                &cancel,
                &mut p,
            )
            .unwrap();
            println!("{:?}", r);
        }
        _ => {}
    }
}
