"""
Common imports for the lxml-derived test suite, adapted to use retree.etree.

Tests that need lxml-specific features (LXML_VERSION, LIBXML_VERSION, etc.)
will be skipped automatically because those attributes are absent from
retree.etree.  The helpers below mirror the ones found in lxml's own
``common_imports.py`` test helper.
"""
from __future__ import annotations

import doctest  # noqa: F401 (re-exported)
import os
import sys
import tempfile
import unittest
from io import BytesIO  # noqa: F401 (re-exported)
from urllib.request import pathname2url

import retree.etree as etree  # noqa: F401 (re-exported)

# Provide the stdlib ElementTree module as a reference implementation for
# comparison tests (lxml test_elementtree.py does "if ElementTree is not None").
try:
    import xml.etree.ElementTree as ElementTree  # noqa: F401 (re-exported)
except ImportError:  # pragma: no cover
    ElementTree = None  # type: ignore[assignment]

# Version string for reporting ("ElementTree X.Y")
ET_VERSION: str = getattr(ElementTree, "VERSION", "unknown")


def filter_by_version(test_class_or_callable=None, required_versions=None, version_string=None):
    """Decorator/function that optionally filters test methods by version.

    Called in three ways by the lxml test suite:
    1. ``filter_by_version(cls, required_versions, version_string)`` – modifies
       *cls* in-place to skip tests that require a higher version.
    2. ``@filter_by_version(...)`` – decorator form.
    3. ``filter_by_version(func)`` – wraps a single callable.

    In retree we never skip on version, so this is essentially a no-op.
    """
    if test_class_or_callable is None:
        return lambda f: f
    # Called as filter_by_version(cls, required_versions, version_string)
    # or filter_by_version(cls) – just return it as-is.
    return test_class_or_callable

# ---------------------------------------------------------------------------
# Constants / compatibility shims
# ---------------------------------------------------------------------------

IS_PYPY: bool = hasattr(sys, "pypy_version_info")

# Canonicalize helper – use our etree's canonicalize if present, else stdlib.
try:
    canonicalize = etree.canonicalize  # type: ignore[attr-defined]
except AttributeError:  # pragma: no cover
    from xml.etree.ElementTree import canonicalize  # type: ignore[assignment]


def _str(s: str) -> str:  # noqa: D401
    """Return *s* unchanged (lxml compat: used to handle py2/py3 string diffs)."""
    return s


def _bytes(s: str) -> bytes:
    """Encode *s* to UTF-8 bytes (lxml compat helper)."""
    return s.encode("utf-8")


# ---------------------------------------------------------------------------
# File / URL helpers
# ---------------------------------------------------------------------------

_TESTS_DIR = os.path.dirname(os.path.abspath(__file__))


def fileInTestDir(name: str) -> str:
    """Return the full path to *name* inside the lxml_tests directory."""
    return os.path.join(_TESTS_DIR, name)


def path2url(path: str) -> str:
    """Convert a filesystem path to a ``file://`` URL."""
    return "file://" + pathname2url(os.path.abspath(path))


def fileUrlInTestDir(name: str) -> str:
    """Return a ``file://`` URL for *name* inside the lxml_tests directory."""
    return path2url(fileInTestDir(name))


def read_file(name: str, mode: str = "rb"):
    """Open *name* relative to the lxml_tests directory and return its contents."""
    with open(fileInTestDir(name), mode) as f:
        return f.read()


class tmpfile:  # noqa: N801 – lxml uses lowercase
    """Context manager that yields a temporary file path and removes it on exit."""

    def __init__(self, suffix: str = ".xml"):
        self._suffix = suffix
        self._path: str | None = None

    def __enter__(self) -> str:
        fd, self._path = tempfile.mkstemp(suffix=self._suffix)
        os.close(fd)
        return self._path

    def __exit__(self, *_) -> None:
        if self._path and os.path.exists(self._path):
            os.unlink(self._path)


# ---------------------------------------------------------------------------
# Feature gating
# ---------------------------------------------------------------------------

def needs_feature(*features: str):
    """Decorator / function that skips a test when lxml features are absent.

    Because retree does not expose lxml feature flags, any test decorated with
    ``@needs_feature(...)`` is unconditionally skipped.
    """
    def decorator(func_or_class):
        reason = "lxml feature(s) not available in retree: " + ", ".join(features)
        return unittest.skip(reason)(func_or_class)
    return decorator


# ---------------------------------------------------------------------------
# File-like helpers used in lxml tests
# ---------------------------------------------------------------------------

class SillyFileLike:
    """A minimal read-only file-like object that wraps bytes."""

    def __init__(self, data: bytes = b"<root/>"):
        self._data = data
        self._pos = 0

    def read(self, n: int = -1) -> bytes:
        if n < 0:
            chunk = self._data[self._pos:]
            self._pos = len(self._data)
        else:
            chunk = self._data[self._pos: self._pos + n]
            self._pos += len(chunk)
        return chunk


class LargeFileLikeUnicode:
    """Yields a large XML document as a unicode stream, one chunk at a time."""

    def __init__(self, chunks: int = 3):
        self._chunks = chunks
        self._count = 0
        self._header_sent = False
        self._footer_sent = False

    def read(self, _n: int = -1) -> str:
        if not self._header_sent:
            self._header_sent = True
            return "<root>"
        if self._count < self._chunks:
            self._count += 1
            return "<child/>" * 100
        if not self._footer_sent:
            self._footer_sent = True
            return "</root>"
        return ""


class SimpleFSPath:
    """Wraps a path string in an object with ``__fspath__`` (PEP 519)."""

    def __init__(self, path: str):
        self.path = path

    def __fspath__(self) -> str:
        return self.path


# ---------------------------------------------------------------------------
# doctest helpers
# ---------------------------------------------------------------------------

def make_doctest(module_name: str) -> doctest.DocTestSuite:  # type: ignore[name-defined]
    """Return a :class:`doctest.DocTestSuite` for *module_name*."""
    return doctest.DocTestSuite(module_name)


# ---------------------------------------------------------------------------
# Base test case
# ---------------------------------------------------------------------------

class HelperTestCase(unittest.TestCase):
    """Base class for lxml-derived tests adapted to retree."""

    def assertXML(self, expected: bytes, element, encoding: str = "utf-8") -> None:
        """Assert that serialising *element* gives *expected* bytes."""
        result = etree.tostring(element, encoding=encoding)
        self.assertEqual(expected, result)

    def assertEncodingDeclaration(self, result: bytes, encoding: str) -> None:
        """Assert the XML declaration in *result* mentions *encoding*."""
        self.assertIn(encoding.lower().encode(), result.lower())
