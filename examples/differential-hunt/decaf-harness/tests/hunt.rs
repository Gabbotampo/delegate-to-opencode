//! Differential hunt harness: runs every row of a probe through the decaf runtime.
//! PROBE=<dir with demo/Probe.java> OUT=<file> [LO] [HI]
//! demo.Probe must have `public static String row(int i)` returning "label=value", or "NOROW" past the end.
//! Each output line: `label=value<TAB>unknown host calls` (empty when the runtime answered).
#[path = "../../decaf/crates/horizon-native-java/tests/support/javac_fixture.rs"]
mod javac_fixture;
use horizon_native_java::{StdHost, Value};
use javac_fixture::Fixture;
use std::io::Write;

/// Same escaping as HuntRunner.java: printable ASCII as is, `\\` doubled, every other UTF-16 unit as \uXXXX.
fn esc(s: &str) -> String {
    let mut b = String::new();
    for u in s.encode_utf16() {
        match u {
            0x5c => b.push_str("\\\\"),
            0x20..=0x7e => b.push(u as u8 as char),
            _ => b.push_str(&format!("\\u{u:04X}")),
        }
    }
    b
}

#[test]
fn run() {
    let dir = std::env::var("PROBE").expect("PROBE dir");
    let src = std::fs::read_to_string(format!("{dir}/demo/Probe.java")).expect("demo/Probe.java");
    let src: &'static str = Box::leak(src.into_boxed_str());
    let lo: i32 = std::env::var("LO").map(|v| v.parse().unwrap()).unwrap_or(0);
    let hi: i32 = std::env::var("HI").map(|v| v.parse().unwrap()).unwrap_or(i32::MAX);
    let fixture = Fixture::build("hunt", &[("demo/Probe.java", src)]);
    let mut f = std::fs::File::create(std::env::var("OUT").expect("OUT")).unwrap();
    for i in lo..hi {
        let mut host = fixture.jar.seed_host(StdHost::new());
        let r = fixture.jar.invoke_method_with_args(
            "demo/Probe", "row", "(I)Ljava/lang/String;", None, &[Value::Int(i)], &mut host);
        let t = match r { Ok(Value::Ref(Some(t))) => t, other => format!("ERR {other:?}") };
        if t == "NOROW" { break; }
        let unknown = host.unknown_host_calls().iter().map(|(k, n)| format!("{k}x{n}")).collect::<Vec<_>>().join(" ; ");
        writeln!(f, "{}\t{}", esc(&t), unknown).unwrap();
        f.flush().unwrap();
    }
}
