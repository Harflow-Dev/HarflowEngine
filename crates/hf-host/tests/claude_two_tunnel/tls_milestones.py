"""Closed native TLS callback milestones; callback payloads are never inspected."""
import ssl

READ_CLIENT_HELLO = 1 << 0
WRITE_SERVER_HELLO = 1 << 1
WRITE_CERTIFICATE = 1 << 2
WRITE_SERVER_KEY_EXCHANGE = 1 << 3
WRITE_SERVER_HELLO_DONE = 1 << 4
READ_CLIENT_KEY_EXCHANGE = 1 << 5
READ_CCS = 1 << 6
READ_FINISHED = 1 << 7
WRITE_CCS = 1 << 8
WRITE_FINISHED = 1 << 9
READ_ALERT = 1 << 10
WRITE_ALERT = 1 << 11


class Observer:
    def __init__(self):
        self.records = {}
        self.failed = False

    def install(self, context):
        try:
            context._msg_callback = self.message
            context.sni_callback = self.sni
            if context._msg_callback != self.message or context.sni_callback != self.sni:
                self.failed = True
        except Exception:
            self.failed = True
        return None

    def register(self, secure, ordinal, peer):
        try:
            if (len(self.records) >= 4 or secure in self.records
                    or type(ordinal) is not int or ordinal != len(self.records) + 1
                    or type(peer) is not tuple or len(peer) != 2
                    or peer[0] != "127.0.0.1" or type(peer[1]) is not int
                    or not 1 <= peer[1] <= 65535 or secure.getpeername() != peer
                    or any(row[1] == peer[1] for row in self.records.values())):
                self.failed = True
            else:
                self.records[secure] = [ordinal, peer[1], 0, "unseen"]
        except Exception:
            self.failed = True
        return None

    def sni(self, secure, server_name, context):
        try:
            row = self.records[secure]
            if row[3] != "unseen":
                self.failed = True
            elif server_name is None:
                row[3] = "absent"
            elif type(server_name) is str:
                row[3] = "matched" if server_name == "api.anthropic.com" else "other"
            else:
                self.failed = True
        except Exception:
            self.failed = True
        return None

    def message(self, secure, direction, version, content_type, message_type, data):
        try:
            row = self.records[secure]
            if type(direction) is not str or direction not in ("read", "write"):
                self.failed = True
            else:
                self._event(row, direction, content_type, message_type)
        except Exception:
            self.failed = True
        return None

    def _event(self, row, direction, content, message):
        ct, mt = ssl._TLSContentType, ssl._TLSMessageType
        if type(content) is not ct:
            self.failed = True
        elif content is ct.HEADER:
            if type(message) is not ct or message not in (
                    ct.CHANGE_CIPHER_SPEC, ct.ALERT, ct.HANDSHAKE, ct.APPLICATION_DATA):
                self.failed = True
        elif content is ct.ALERT:
            if type(message) is ssl._TLSAlertType:
                row[2] |= READ_ALERT if direction == "read" else WRITE_ALERT
            else:
                self.failed = True
        elif content is ct.CHANGE_CIPHER_SPEC:
            if type(message) is mt and message is mt.CHANGE_CIPHER_SPEC:
                row[2] |= READ_CCS if direction == "read" else WRITE_CCS
            else:
                self.failed = True
        elif content is ct.HANDSHAKE and type(message) is mt:
            self._handshake(row, direction, message, mt)
        else:
            self.failed = True

    def _handshake(self, row, direction, message, mt):
        events = {
            ("read", mt.CLIENT_HELLO): READ_CLIENT_HELLO,
            ("write", mt.SERVER_HELLO): WRITE_SERVER_HELLO,
            ("write", mt.CERTIFICATE): WRITE_CERTIFICATE,
            ("write", mt.SERVER_KEY_EXCHANGE): WRITE_SERVER_KEY_EXCHANGE,
            ("write", mt.SERVER_DONE): WRITE_SERVER_HELLO_DONE,
            ("read", mt.CLIENT_KEY_EXCHANGE): READ_CLIENT_KEY_EXCHANGE,
            ("read", mt.FINISHED): READ_FINISHED,
            ("write", mt.FINISHED): WRITE_FINISHED,
        }
        bit = events.get((direction, message))
        if bit is not None:
            row[2] |= bit
        elif direction == "write" and message is mt.NEWSESSION_TICKET:
            pass
        else:
            self.failed = True

    def emit(self):
        if self.failed:
            print("MILESTONE_ERROR", flush=True)
        else:
            for ordinal, port, mask, sni in self.records.values():
                print(f"TLS_MILESTONE_V1\t{ordinal}\t127.0.0.1\t{port}\t{mask}\t{sni}", flush=True)
