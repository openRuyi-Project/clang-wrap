"""Mock's plugin-name-compatible loader for mock-llvmir.py."""

from pathlib import Path
import importlib.util

_path = Path(__file__).with_name("mock-llvmir.py")
_spec = importlib.util.spec_from_file_location("mock_llvmir_impl", _path)
_module = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_module)

requires_api_version = _module.requires_api_version
run_in_bootstrap = _module.run_in_bootstrap
init = _module.init