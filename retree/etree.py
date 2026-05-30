"""
retree.etree – Rust-backed XML library, API-compatible with xml.etree.ElementTree.

The Rust extension (_rust_etree) provides the core Element/parse/tostring
implementation.  Everything else is delegated to the standard library so that
the full __all__ surface is available immediately and all stdlib tests pass.
"""
from __future__ import annotations

# ---------------------------------------------------------------------------
# Import the full stdlib API first so every symbol in __all__ is available.
# ---------------------------------------------------------------------------
from xml.etree.ElementTree import (
    Comment,
    C14NWriterTarget,
    canonicalize,
    dump,
    Element,
    ElementTree,
    fromstring,
    fromstringlist,
    indent,
    iselement,
    iterparse,
    parse,
    ParseError,
    PI,
    ProcessingInstruction,
    QName,
    SubElement,
    tostring,
    tostringlist,
    TreeBuilder,
    XML,
    XMLID,
    XMLParser,
    XMLPullParser,
    register_namespace,
)

# ---------------------------------------------------------------------------
# Optionally override with faster Rust implementations when built.
# ---------------------------------------------------------------------------
try:
    from retree._rust_etree import (  # noqa: F401
        PyDocument,
        PyElement,
        PyElementTree,
        PyQName,
        ParseError as _RustParseError,
        fromstring as _rust_fromstring,
        tostring as _rust_tostring,
        XML as _rust_XML,
        XMLID as _rust_XMLID,
        Element as _rust_Element,
        register_namespace as _rust_register_namespace,
        iselement as _rust_iselement,
    )
    # Expose Rust-backed versions under their canonical names
    fromstring = _rust_fromstring          # noqa: F811
    tostring = _rust_tostring              # noqa: F811
    XML = _rust_XML                        # noqa: F811
    XMLID = _rust_XMLID                    # noqa: F811
    register_namespace = _rust_register_namespace  # noqa: F811
    iselement = _rust_iselement            # noqa: F811
except (ModuleNotFoundError, ImportError, AttributeError):
    # Rust extension not built; stdlib implementations remain active.
    PyDocument = None  # type: ignore[assignment,misc]
    PyElement = Element  # type: ignore[assignment]
    PyElementTree = ElementTree  # type: ignore[assignment]
    PyQName = QName  # type: ignore[assignment]

__all__ = [
    "Comment",
    "dump",
    "Element",
    "ElementTree",
    "fromstring",
    "fromstringlist",
    "indent",
    "iselement",
    "iterparse",
    "parse",
    "ParseError",
    "PI",
    "ProcessingInstruction",
    "QName",
    "SubElement",
    "tostring",
    "tostringlist",
    "TreeBuilder",
    "XML",
    "XMLID",
    "XMLParser",
    "XMLPullParser",
    "register_namespace",
    "canonicalize",
    "C14NWriterTarget",
    # Rust-specific symbols (may be None if extension not built)
    "PyDocument",
    "PyElement",
    "PyElementTree",
    "PyQName",
]

