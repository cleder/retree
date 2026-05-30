# retree

retree is a high-performance XML library designed as a modern, drop-in alternative to `xml.etree.ElementTree` and `lxml.etree`.
Built with Rust and PyO3, it aims to provide a fast, memory-efficient, and reliable solution for XML processing while maintaining a familiar, Pythonic API.

## Installation and import goal

```bash
uv add retree
```

```python
from retree import etree as ET
```

## Repository scaffold

This repository now uses a mixed Rust/Python layout compatible with `maturin`:

- `src/lib.rs`: PyO3 extension entrypoint (`retree._rust_etree`)
- `retree/`: Python re-export package
- `tests/test_rust_etree_compat.py`: stdlib-accelerator harness (`sys.modules['_elementtree'] = _rust_etree`)

## Development loop

```bash
# Build extension in the active virtualenv
maturin develop

# Run targeted compatibility checks
python -m unittest discover -s tests -p 'test_*.py'
```
