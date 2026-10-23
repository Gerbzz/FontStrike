//! Regression guards for the approved product logo, without renderer or dependency changes.

use sha2::{Digest, Sha256};

const PRODUCT_LOGO: &[u8] = include_bytes!("../assets/branding/FontStrike-logo.png");
const APPROVED_SHA256: &str = "0974b816f647d35cdb66d797b0e0e3b365d12fef891de2197dc2aaa2081779c3";

#[test]
fn approved_product_logo_keeps_its_original_rgba_png_bytes() {
    assert!(PRODUCT_LOGO.len() >= 33, "the PNG must contain its IHDR");
    assert_eq!(&PRODUCT_LOGO[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&PRODUCT_LOGO[12..16], b"IHDR");
    assert_eq!(u32::from_be_bytes(PRODUCT_LOGO[16..20].try_into().unwrap()), 1024);
    assert_eq!(u32::from_be_bytes(PRODUCT_LOGO[20..24].try_into().unwrap()), 1024);
    assert_eq!(PRODUCT_LOGO[24], 8, "eight-bit channels");
    assert_eq!(PRODUCT_LOGO[25], 6, "RGBA, including the approved transparency");
    assert_eq!(format!("{:x}", Sha256::digest(PRODUCT_LOGO)), APPROVED_SHA256);

    let provenance: serde_json::Value =
        serde_json::from_str(include_str!("../assets/branding/FontStrike-logo.provenance.json"))
            .unwrap();
    assert_eq!(provenance["assets"]["png"]["sha256"], APPROVED_SHA256);
    assert_eq!(provenance["assets"]["png"]["dimensions"], serde_json::json!([1024, 1024]));
}

#[test]
fn native_start_about_and_window_icon_share_the_approved_product_asset() {
    let desktop = include_str!("../src/bin/desktop.rs");
    assert!(desktop.contains(
        "const LOGO_PNG: &[u8] = include_bytes!(\"../../assets/branding/FontStrike-logo.png\");"
    ));
    assert!(!desktop.contains("assets/branding/FontStrike.png"));
    assert!(!desktop.to_ascii_lowercase().contains("woodpecker"));
    assert!(desktop.contains("self.logo = decode_png(LOGO_PNG, 220)"));
    assert!(desktop.contains("if let Some(icon) = decode_png(LOGO_PNG, 64)"));
    assert_eq!(desktop.matches("if let Some(logo) = &self.logo").count(), 2);
    assert_eq!(desktop.matches("\"FontStrike product logo\"").count(), 2);
    assert!(desktop.contains(
        "const PWDS_PNG: &[u8] = include_bytes!(\"../../assets/branding/PWDS-logo.png\");"
    ));
    assert!(desktop.contains("self.pwds = decode_png(PWDS_PNG, 400)"));
}
