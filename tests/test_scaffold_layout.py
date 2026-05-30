import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


class ScaffoldLayoutTests(unittest.TestCase):
    def test_expected_project_files_exist(self) -> None:
        expected = [
            ROOT / "Cargo.toml",
            ROOT / "pyproject.toml",
            ROOT / "src" / "lib.rs",
            ROOT / "rust_etree" / "__init__.py",
            ROOT / "rust_etree" / "etree.py",
            ROOT / "tests" / "test_rust_etree_compat.py",
        ]
        for path in expected:
            with self.subTest(path=path):
                self.assertTrue(path.exists(), f"missing file: {path}")

    def test_cargo_targets_python_extension_module(self) -> None:
        cargo_toml = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn('crate-type = ["cdylib"]', cargo_toml)
        self.assertIn('pyo3 = { version = "0.28.3", features = ["extension-module"] }', cargo_toml)


if __name__ == "__main__":
    unittest.main()
