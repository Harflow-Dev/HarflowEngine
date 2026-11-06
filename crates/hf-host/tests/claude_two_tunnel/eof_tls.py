"""Four-session diagnostic wrapper retaining the original wave TLS behavior."""
import importlib.util
import os


def load(name, filename):
    spec = importlib.util.spec_from_file_location(
        name, os.path.join(os.path.dirname(__file__), filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


wave = load("harflow_eof_wave", "wave_tls.py")
milestones = load("harflow_tls_milestones", "tls_milestones.py")


def serve(context):
    observer = milestones.Observer()
    observer.install(context)
    wave.serve_wave(context, observer.register)
    observer.emit()


if __name__ == "__main__":
    raise SystemExit(wave.base.main(serve))
