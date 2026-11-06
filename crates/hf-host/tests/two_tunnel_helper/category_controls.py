"""Exact structured TLS reason mapping controls; fake exception cases are table-only."""
import importlib.util
import contextlib
import io
import ssl
import sys


class StructuredSSLError(ssl.SSLError):
    def __init__(self, library, reason, message):
        super().__init__(message)
        self.library = library
        self.reason = reason


def load_classifier(path):
    spec = importlib.util.spec_from_file_location("offline_tls_helper", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.classify_error


def verify(path):
    classify = load_classifier(path)

    def classify_quietly(error):
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            result = classify(error)
        if stdout.getvalue() or stderr.getvalue():
            raise AssertionError("classifier emitted error data")
        return result

    cases = {
        "TLSV1_ALERT_UNKNOWN_CA": "certificate_alert_unknown_ca",
        "TLSV1_ALERT_BAD_CERTIFICATE": "certificate_alert_bad_certificate",
        "SSLV3_ALERT_BAD_CERTIFICATE": "certificate_alert_bad_certificate",
        "SSLV3_ALERT_CERTIFICATE_UNKNOWN": "certificate_alert_certificate_unknown",
        "TLSV1_ALERT_CERTIFICATE_UNKNOWN": "certificate_alert_certificate_unknown",
        "TLSV1_ALERT_INTERNAL_ERROR": "peer_alert_internal_error",
        "TLSV1_ALERT_PROTOCOL_VERSION": "peer_alert_protocol_version",
        "SSLV3_ALERT_HANDSHAKE_FAILURE": "peer_alert_handshake_failure",
        "TLSV1_ALERT_DECODE_ERROR": "peer_alert_decode_error",
        "SSLV3_ALERT_ILLEGAL_PARAMETER": "peer_alert_illegal_parameter",
        "WRONG_VERSION_NUMBER": "protocol_wrong_version",
        "UNKNOWN_PROTOCOL": "protocol_unrecognized",
        "NO_SHARED_CIPHER": "protocol_no_shared_cipher",
        "UNSUPPORTED_PROTOCOL": "protocol_unsupported_protocol",
    }
    for reason, category in cases.items():
        result = classify_quietly(StructuredSSLError("SSL", reason, "opaque-secret-payload"))
        expected_status = "cert_alert" if category.startswith("certificate_alert_") else "tls_error"
        if result != (expected_status, category, expected_status == "cert_alert"):
            raise AssertionError("exact reason table mismatch")
    for library, reason in (
        ("OTHER", "TLSV1_ALERT_UNKNOWN_CA"),
        ("SSL", "TLSV1_ALERT_UNKNOWN_CA_extra"),
        ("SSL", "opaque-secret-payload"),
        (None, "TLSV1_ALERT_BAD_CERTIFICATE"),
        ("SSL", None),
        ("SSL", 7),
    ):
        result = classify_quietly(
            StructuredSSLError(library, reason, "opaque-secret-payload")
        )
        if result != ("tls_error", "tls_unclassified", False):
            raise AssertionError("unrecognized TLS structure was not unknown")
    for error in (ssl.SSLError("opaque-secret-payload"), OSError("opaque-secret-payload")):
        result = classify_quietly(error)
        if "opaque-secret-payload" in repr(result) or result[2]:
            raise AssertionError("error text escaped or produced certificate verdict")
    return 0


def verify_prehello_fixtures(helper_script):
    controls_path = __file__.replace("category_controls.py", "helper_controls.py")
    spec = importlib.util.spec_from_file_location("offline_tls_helper_controls", controls_path)
    controls = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(controls)
    with controls.tempfile.TemporaryDirectory(prefix="hf-two-tunnel-alerts-") as directory:
        with controls.Helper(helper_script, directory) as helper:
            for alert in (b"\x15\x03\x03\x00\x02\x02\x50", b"\x15\x03\x03\x00\x02\x02\x46"):
                peer = controls.socket.create_connection(("127.0.0.1", helper.port), timeout=5)
                peer.sendall(alert)
                peer.close()
            sessions = [helper.session(5), helper.session(5)]
            helper.finish()
    if any(
        item["phase"] != "handshake"
        or item["status"] != "tls_error"
        or item["certificate"]
        or item["category"] != "protocol_unrecognized"
        for item in sessions
    ):
        raise AssertionError("pre-ClientHello fixture was not classified as unrecognized protocol")
    for fixture, item in zip(("fatal_alert_bytes_a", "fatal_alert_bytes_b"), sessions):
        print("OBSERVED\t{}\t{}\t{}".format(fixture, item["phase"], item["category"]))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(2)
    try:
        verify(sys.argv[1])
        print("CONTROL\tstructured_reason_allowlist_table_only\tpass")
    except Exception:
        print("CONTROL\tstructured_reason_allowlist_table_only\tfail")
        raise SystemExit(1)
