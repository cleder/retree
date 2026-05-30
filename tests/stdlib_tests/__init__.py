"""
Compatibility shims for running Python 3.13-era stdlib tests on Python 3.12.

Importing this package applies the necessary monkey-patches to ``test.support``
so that tests using ``@support.subTests(...)`` and
``@support.skip_wasi_stack_overflow()`` can at least be loaded and (where
possible) executed.
"""
import sys
import unittest

# Only patch on Python < 3.13
if sys.version_info < (3, 13):
    import test.support as _support

    # ------------------------------------------------------------------
    # support.subTests – introduced in Python 3.13 (bpo-110099)
    # Provides a decorator that runs a test function once per set of
    # parameters, similar to @pytest.mark.parametrize.
    # ------------------------------------------------------------------
    if not hasattr(_support, "subTests"):
        def _subTests(arg_name, arg_values, /, **kw):  # noqa: N802
            """Minimal backport of test.support.subTests for Python 3.12."""
            if isinstance(arg_name, str) and "," not in arg_name:
                # Single parameter: values are plain scalars.
                param_names = [arg_name.strip()]
            else:
                # Multiple parameters: "name1,name2" with tuple values.
                param_names = [n.strip() for n in arg_name.split(",")]

            def decorator(func):
                def wrapper(self):
                    for values in arg_values:
                        if not isinstance(values, tuple):
                            values = (values,)
                        kwargs = dict(zip(param_names, values))
                        with self.subTest(**kwargs):
                            func(self, **kwargs)
                wrapper.__name__ = func.__name__
                wrapper.__qualname__ = func.__qualname__
                return wrapper
            return decorator

        _support.subTests = _subTests  # type: ignore[attr-defined]

    # ------------------------------------------------------------------
    # support.skip_wasi_stack_overflow – introduced in Python 3.13
    # On non-WASI platforms this is a no-op decorator.
    # ------------------------------------------------------------------
    if not hasattr(_support, "skip_wasi_stack_overflow"):
        def _skip_wasi_stack_overflow():  # noqa: N802
            """Backport stub: no-op on non-WASI platforms."""
            return lambda f: f

        _support.skip_wasi_stack_overflow = _skip_wasi_stack_overflow  # type: ignore[attr-defined]

    if not hasattr(_support, "skip_emscripten_stack_overflow"):
        def _skip_emscripten_stack_overflow():  # noqa: N802
            """Backport stub: no-op on non-Emscripten platforms."""
            return lambda f: f

        _support.skip_emscripten_stack_overflow = _skip_emscripten_stack_overflow  # type: ignore[attr-defined]

    if not hasattr(_support, "skip_if_unlimited_stack_size"):
        _support.skip_if_unlimited_stack_size = lambda f: f  # type: ignore[attr-defined]

    # ------------------------------------------------------------------
    # unittest.TestCase.assertHasAttr / assertNotHasAttr – Python 3.13+
    # ------------------------------------------------------------------
    if not hasattr(unittest.TestCase, "assertHasAttr"):
        def _assertHasAttr(self, obj, name, msg=None):  # noqa: N802
            if not hasattr(obj, name):
                std_msg = f"{obj!r} does not have attribute {name!r}"
                self.fail(self._formatMessage(msg, std_msg))

        def _assertNotHasAttr(self, obj, name, msg=None):  # noqa: N802
            if hasattr(obj, name):
                std_msg = f"{obj!r} unexpectedly has attribute {name!r}"
                self.fail(self._formatMessage(msg, std_msg))

        unittest.TestCase.assertHasAttr = _assertHasAttr  # type: ignore[attr-defined]
        unittest.TestCase.assertNotHasAttr = _assertNotHasAttr  # type: ignore[attr-defined]
