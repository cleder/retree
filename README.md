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

## Public Interface

python stdlib:

```python
__all__ = [
    # public symbols
    "Comment",
    "dump",
    "Element", "ElementTree",
    "fromstring", "fromstringlist",
    "indent", "iselement", "iterparse",
    "parse", "ParseError",
    "PI", "ProcessingInstruction",
    "QName",
    "SubElement",
    "tostring", "tostringlist",
    "TreeBuilder",
    "XML", "XMLID",
    "XMLParser", "XMLPullParser",
    "register_namespace",
    "canonicalize", "C14NWriterTarget",
    ]
```

lxml:

```python
__all__ = [
    'AttributeBasedElementClassLookup', 'C14NError', 'C14NWriterTarget', 'CDATA',
    'Comment', 'CommentBase', 'CustomElementClassLookup', 'DEBUG',
    'DTD', 'DTDError', 'DTDParseError', 'DTDValidateError',
    'DocumentInvalid', 'ETCompatXMLParser', 'ETXPath', 'Element',
    'ElementBase', 'ElementClassLookup', 'ElementDefaultClassLookup',
    'ElementNamespaceClassLookup', 'ElementTree', 'Entity', 'EntityBase',
    'Error', 'ErrorDomains', 'ErrorLevels', 'ErrorTypes', 'Extension',
    'FallbackElementClassLookup', 'FunctionNamespace', 'HTML', 'HTMLParser',
    'ICONV_COMPILED_VERSION',
    'LIBXML_COMPILED_VERSION', 'LIBXML_VERSION',
    'LIBXML_FEATURES',
    'LIBXSLT_COMPILED_VERSION', 'LIBXSLT_VERSION',
    'LXML_VERSION',
    'LxmlError', 'LxmlRegistryError', 'LxmlSyntaxError',
    'NamespaceRegistryError', 'PI', 'PIBase', 'ParseError',
    'ParserBasedElementClassLookup', 'ParserError', 'ProcessingInstruction',
    'PyErrorLog', 'PythonElementClassLookup', 'QName', 'RelaxNG',
    'RelaxNGError', 'RelaxNGErrorTypes', 'RelaxNGParseError',
    'RelaxNGValidateError', 'Resolver', 'Schematron', 'SchematronError',
    'SchematronParseError', 'SchematronValidateError', 'SerialisationError',
    'SubElement', 'TreeBuilder', 'XInclude', 'XIncludeError', 'XML',
    'XMLDTDID', 'XMLID', 'XMLParser', 'XMLSchema', 'XMLSchemaError',
    'XMLSchemaParseError', 'XMLSchemaValidateError', 'XMLSyntaxError',
    'XMLTreeBuilder', 'XPath', 'XPathDocumentEvaluator', 'XPathError',
    'XPathEvalError', 'XPathEvaluator', 'XPathFunctionError', 'XPathResultError',
    'XPathSyntaxError', 'XSLT', 'XSLTAccessControl', 'XSLTApplyError',
    'XSLTError', 'XSLTExtension', 'XSLTExtensionError', 'XSLTParseError',
    'XSLTSaveError', 'canonicalize',
    'cleanup_namespaces', 'clear_error_log', 'dump',
    'fromstring', 'fromstringlist', 'get_default_parser', 'iselement',
    'iterparse', 'iterwalk', 'parse', 'parseid', 'register_namespace',
    'set_default_parser', 'set_element_class_lookup', 'strip_attributes',
    'strip_elements', 'strip_tags', 'tostring', 'tostringlist', 'tounicode',
    'use_global_python_log'
    ]
```

The goal is to provide a minimal interface compatible with the std library.

## Implementation Strategy

An implementation strategy designed to replace `lxml` with a Rust-backed core must address the two hardest parts of this project: **memory safety at the Python-Rust boundary (handling parent/child tree references)** and **rigorous API parity**.

By adopting a Test-Driven Development (TDD) loop that hijacks the extensive test suites of both the Python Standard Library (`xml.etree`) and `lxml`, you can verify your implementation correctness incrementally.

Below is a step-by-step implementation strategy.

---

### Step 1: Scaffolding and Build Environment

To build a high-performance, distributable Python extension, use a **mixed Rust/Python repository structure** managed by `maturin`.

1. **Initialize the Project**:
Create a mixed layout so that you can write high-level glue code in Python and heavy systems-level code in Rust.


```bash
pip install maturin
maturin init --bindings pyo3

```


2. **Directory Layout**:
Configure the repository to expose a clean package structure :
rust_etree/
├── Cargo.toml
├── pyproject.toml
├── rust_etree/            # Python wrapper package
│   ├── **init**.py
│   └── etree.py           # Re-exports Rust classes + Python fallback helpers
├── src/
│   └── lib.rs             # PyO3 Rust extension entry point
└── tests/                 # Reused test suites


3. **Build Configuration (`Cargo.toml`)**:
Ensure you compile as a C-compatible dynamic library (`cdylib`) so Python can load it. Enable the `extension-module` feature in `pyo3` :


```toml
[lib]
name = "_rust_etree"
crate-type = ["cdylib"]

[dependencies]
pyo3 = { version = "0.28.3", features = ["extension-module"] }
xmloxide = "0.4.3"  # Core parsing, mutation, and XPath engine

```



---

### Step 2: The Core Architectural Pattern (Proxy-Arena)

In Python, the `Element` class in `xml.etree` or `lxml` behaves like a mutable, fully navigable node. In Rust, modeling parent-child relationships using standard references creates pointer-aliasing issues and circular reference leaks.

To solve this, implement the **Proxy-Arena Pattern** inspired by `ast-grep`’s PyO3 implementation :

* **`PyDocument`**: A single Rust struct that owns the underlying XML tree (the "Arena").


* **`PyElement`**: A lightweight Python-managed proxy struct that holds a pointer (`Py<PyDocument>`) to the document and a stable `NodeId` integer representing the node in the arena.



#### Conceptual Rust Implementation (`src/lib.rs`):

```rust
use pyo3::prelude::*;
use xmloxide::tree::{Document, NodeId}; // Utilizing xmloxide's safe arena [8]

#[pyclass(subclass)]
pub struct PyDocument {
    pub tree: Document,
}

#[pyclass(subclass)]
#[derive(Clone)]
pub struct PyElement {
    pub node_id: NodeId,
    pub doc_ref: Py<PyDocument>, // Strong reference prevents the document from being GC'd [6]
}

#[pymethods]
impl PyElement {
    // Implements lxml's getparent()
    fn getparent(&self, py: Python) -> PyResult<Option<PyElement>> {
        let doc = self.doc_ref.borrow(py);
        match doc.tree.parent(self.node_id) {
            Some(parent_id) => Ok(Some(PyElement {
                node_id: parent_id,
                doc_ref: self.doc_ref.clone_ref(py),
            })),
            None => Ok(None),
        }
    }
}

```

---

### Step 3: Test Harness Configuration (The TDD Setup)

Before writing any core parsing code, set up your test runner to execute the CPython standard library tests and `lxml` tests against your native module.

#### 1. Import Python's Standard Library Tests

The Python standard library includes `test_xml_etree.py` and `test_xml_etree_c.py`. These tests are designed to run against both the pure Python implementation and the accelerated C module (`_elementtree`).

These test are located in `tests/stdlib_tests/`

Create a test harness in your `tests/` directory:

```python
# tests/test_rust_etree_compat.py
import sys
import unittest

# 1. Build and import your custom compiled Rust module [2]
import _rust_etree 

# 2. Mock or swap the standard library's C accelerator with your Rust module
sys.modules['_elementtree'] = _rust_etree

# 3. Import the standard library's test cases
from test.test_xml_etree import * 

if __name__ == '__main__':
    unittest.main()

```

#### 2. Import `lxml`'s Extended Tests

Clone the `lxml` repository and extract `src/lxml/tests/test_etree.py` and `test_elementtree.py`.
Point your test suite to import your `rust_etree` package as the `etree` module.

These tests are located in `tests/lxml_tests/`

#### 3. Run the Iterative Dev Loop

Use `maturin develop` to compile changes incrementally and run `pytest` :

```bash
# Compile and install inside virtual environment
maturin develop

# Run the test suite (expect 99% failures initially)
pytest tests/test_rust_etree_compat.py

```

---

### Step 4: Incremental Implementation Milestones

Use your newly established test harness to guide development across four clear milestones.

```
       +-------------------------------------------------------------+
       |                  MILESTONE 1: READ/WRITE                     |
       |  Implement: fromstring(), tostring(), XMLParser             |
       |  Verifies: Basic serialization/deserialization passes       |
       +------------------------------------+------------------------+
                                            |
                                            v
       +-------------------------------------------------------------+
       |                  MILESTONE 2: NAVIGATION                    |
       |  Implement: getparent(), getnext(), Sequence protocol       |
       |  Verifies: Indexing and axis-traversal tests pass           |
       +------------------------------------+------------------------+
                                            |
                                            v
       +-------------------------------------------------------------+
       |                  MILESTONE 3: MUTABILITY                    |
       |  Implement: append(), remove(), insert(), attrib dictionary |
       |  Verifies: Tree manipulation & state tests pass             |
       +------------------------------------+------------------------+
                                            |
                                            v
       +-------------------------------------------------------------+
       |                    MILESTONE 4: XPATH                       |
       |  Implement: find(), findall(), xpath()                      |
       |  Verifies: Compliance with W3C XPath axis tests             |
       +-------------------------------------------------------------+

```

#### Milestone 1: Basic Serialization (`fromstring` & `tostring`)

Focus first on reading XML bytes and rendering them back into strings.

* Map `_rust_etree.fromstring` to compile input strings using `xmloxide::Document::parse_str`.


* Map `_rust_etree.tostring` to use `xmloxide::serial::serialize`.


* *Target tests to pass*: Basic parsing syntax errors, blank document detection, and simple node serialization.

#### Milestone 2: Basic Tree Navigation

Implement the sequence protocols in PyO3 to allow Python list-like behavior on child nodes.

* Implement `__len__`, `__getitem__` (supporting indexes/slices), and the custom `lxml` extension methods `getparent()`, `getnext()`, and `getprevious()`.
* *Target tests to pass*: Child count, child slicing, and relative element lookups.

#### Milestone 3: Mutability and Attribute Dictionary

Implement tree modification capabilities.

* Wrap `xmloxide`’s mutability methods `append_child()`, `insert_before()`, and `remove_node()` inside PyO3 functions.


* Implement `__setitem__` and `__delitem__` on the element class to mutate child arrays dynamically.


* Replicate the Element `attrib` dictionary using a PyO3 mapping proxy that forwards `.get()`, `.set()`, and key assignments directly to the underlying document's attribute storage.



#### Milestone 4: XPath & Wildcard Matching

Integrate querying mechanisms.

* `xml.etree` expects `.find()`, `.findall()`, and `.findtext()` via `_elementpath.py`. Map these to `xmloxide`'s compliant XPath 1.0 engine.


* Map `lxml`'s custom `.xpath()` method to return a `PyResult<Vec<PyObject>>` that automatically maps matched nodes to `PyElement` proxies.


* Implement the custom namespace wildcard matching conventions `{namespace}*` and `{}tag`.

---

### Step 5: Continuous Integration and Release Pipeline

Once your test suite reaches stable parity, leverage `maturin` to distribute your binary extension.

1. **Automate the Build**: Use the command `maturin generate-ci -m Cargo.toml` to output a GitHub Actions workflow.


2. **Setup Cross-Compiling**: Ensure the `before-script-linux` block installs the necessary library targets inside the "manylinux" Docker containers to produce ready-to-use Python wheels for Linux (x86_64 and AArch64), macOS, and Windows.