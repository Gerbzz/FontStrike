"""Approved 2026-10-25 branding contract. Requires Pillow; does not modify files."""
from pathlib import Path
from hashlib import sha256
import json
import unittest
from PIL import Image
ROOT = Path(__file__).resolve().parents[1]
APP = 'FontStrike'
APPROVED = '8acd8a7dde782193946300cba703a6c4864e37d8d7e6829e0df5b184359c44b6'
class ApprovedBranding(unittest.TestCase):
    def test_exact_approved_source_and_readme(self):
        p = ROOT / 'assets/branding' / (APP + '.png')
        self.assertEqual(sha256(p.read_bytes()).hexdigest(), APPROVED)
        with Image.open(p) as image:
            self.assertEqual(image.size, (512,512))
        self.assertIn('assets/branding/' + APP + '.png', (ROOT/'README.md').read_text())
        self.assertEqual(json.loads((p.parent/'provenance.json').read_text())['sha256'], APPROVED)

    def test_runtime_uses_approved_source(self):
        code = (ROOT/'src/bin/desktop.rs').read_text()
        self.assertIn('include_bytes!("../../assets/branding/FontStrike.png")', code)
        self.assertIn('decode_png(LOGO_PNG, 64)', code)
        self.assertIn('viewport.with_icon(egui::IconData', code)

if __name__ == "__main__":
    unittest.main()
