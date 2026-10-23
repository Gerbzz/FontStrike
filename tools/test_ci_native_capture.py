"""In-memory/temporary PNG validator fixtures only; never application screenshots or artifacts."""
import pathlib
import random
import tempfile
import unittest

from PIL import Image
from ci_native_capture import inspect_pixels, validate_native_png


class NativePngGateTests(unittest.TestCase):
    def test_uniform_frame_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'rendered detail'):
            inspect_pixels(Image.new('RGB', (900, 600), (20, 20, 20)))

    def test_undersized_frame_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'minimum'):
            inspect_pixels(Image.new('RGB', (899, 600), (20, 20, 20)))

    def test_real_png_decoding_records_detail_and_hash(self):
        # Deterministic noise is unit-test input for the image gate, not a GUI capture.
        pixels = random.Random(7).randbytes(900 * 600 * 3)
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'unit-validator-fixture.png'
            Image.frombytes('RGB', (900, 600), pixels).save(path)
            result = validate_native_png(path)
        self.assertEqual((result.get('width'), result.get('height')), (900, 600))
        self.assertGreaterEqual(result.get('sampled_colors', 0), 32)
        self.assertEqual(len(result.get('sha256', '')), 64)

    def test_non_png_payload_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'unit-invalid.png'
            path.write_bytes(b'not a PNG' * 1500)
            with self.assertRaisesRegex(ValueError, 'PNG'):
                validate_native_png(path)


if __name__ == '__main__':
    unittest.main()
