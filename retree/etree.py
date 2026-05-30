try:
    from ._rust_etree import PyDocument, PyElement, fromstring, tostring
except ModuleNotFoundError:
    from xml.etree.ElementTree import Element as PyElement
    from xml.etree.ElementTree import fromstring, tostring

    class PyDocument:  # pragma: no cover - compatibility fallback
        pass

__all__ = [
    "PyDocument",
    "PyElement",
    "fromstring",
    "tostring",
]
