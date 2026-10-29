//! Companion to IV's fake-controller integration test. Never enables output.
//! Build with `cargo build -p trkr-transport --example shared_bus_probe` and
//! pass its path as KOHDALAB_TRKR_SHARED_PROBE to the IV test harness.
fn main() {
    assert_eq!(
        std::env::var("KOHDALAB_HUB_FAKE_CHILD").as_deref(),
        Ok("1"),
        "Run only from the fake-controller integration harness"
    );
    let mut gpib = trkr_transport::Gpib::connect(7).expect("connect shared TRKR");
    for _ in 0..30 {
        assert_eq!(gpib.query("OUTP?\n").unwrap(), "TRKR_TEST_REPLY");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    drop(gpib);
    println!("actual TRKR transport passed");
}
