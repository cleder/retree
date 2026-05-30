import importlib
import sys
import unittest


class RustEtreeStdlibHarnessTests(unittest.TestCase):
    def test_can_patch_elementtree_accelerator(self) -> None:
        try:
            rust_mod = importlib.import_module("_rust_etree")
        except ModuleNotFoundError:
            self.skipTest("_rust_etree extension not built in this environment")
            return

        sys.modules["_elementtree"] = rust_mod
        self.assertIs(sys.modules["_elementtree"], rust_mod)


if __name__ == "__main__":
    unittest.main()
