import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


class ScaffoldLayoutTests(unittest.TestCase):
    def test_expected_project_files_exist(self) -> None:
        expected = [
            ROOT / "Cargo.toml",
            ROOT / "pyproject.toml",
            ROOT / "src" / "lib.rs",
            ROOT / "retree" / "__init__.py",
            ROOT / "retree" / "etree.py",
            ROOT / "tests" / "test_rust_etree_compat.py",
        ]
        for path in expected:
            with self.subTest(path=path):
                self.assertTrue(path.exists(), f"missing file: {path}")

    def test_cargo_targets_python_extension_module(self) -> None:
        cargo_toml = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn('crate-type = ["cdylib"]', cargo_toml)
        self.assertIn('pyo3 = { version = "0.28.3", features = ["extension-module"] }', cargo_toml)

    def test_pyproject_targets_retree_package(self) -> None:
        pyproject = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
        self.assertIn('name = "retree"', pyproject)
        self.assertIn('module-name = "retree._rust_etree"', pyproject)


if __name__ == "__main__":
    unittest.main()
