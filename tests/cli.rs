use std::process::Command;

#[test]
fn command_line_creates_editable_ufo_with_metrics_and_pair_kerning() {
    let exe = env!("CARGO_BIN_EXE_fontstrike");
    let path = std::env::temp_dir().join(format!("fontstrike-cli-{}.ufo", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    for args in [
        vec![
            "new".to_string(),
            path.display().to_string(),
            "Example".into(),
            "Regular".into(),
        ],
        vec![
            "rect".to_string(),
            path.display().to_string(),
            "A".into(),
            "A".into(),
            "620".into(),
            "100".into(),
            "0".into(),
            "520".into(),
            "700".into(),
        ],
        vec![
            "rect".to_string(),
            path.display().to_string(),
            "V".into(),
            "V".into(),
            "600".into(),
            "100".into(),
            "0".into(),
            "500".into(),
            "700".into(),
        ],
        vec![
            "kern".to_string(),
            path.display().to_string(),
            "A".into(),
            "V".into(),
            "-80".into(),
        ],
    ] {
        let output = Command::new(exe).args(&args).output().unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let font = fontstrike::FontProject::open(&path).unwrap();
    assert_eq!(font.width("A").unwrap(), 620.0);
    assert_eq!(font.kerning("A", "V"), Some(-80.0));
}
