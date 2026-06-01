use std::process::Command;

#[test]
fn test_version_flag() {
    let output = Command::new("cargo")
        .args(["run", "--", "--version"])
        .output()
        .expect("Failed to execute");
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("deckdriod"));
}

#[test]
fn test_help_flag() {
    let output = Command::new("cargo")
        .args(["run", "--", "--help"])
        .output()
        .expect("Failed to execute");
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("DeckDriod") || stdout.contains("help"));
}
