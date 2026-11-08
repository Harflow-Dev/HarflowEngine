"""Cursor texts for the verdict tables of `interaction-events` and `statistics-page`.

Each case is a label and a cursor text built from stdlib json and base64 alone. The verdicts of
the reference engine are recorded as literals in the tests that use these cases; they were
derived once by running `_parse` of interaction_events.py and `_decode_cursor` of statistics.py of
HarflowMCP commit a426c5a (read with `git show`, imported with python3 3.13.5) on exactly these texts.
"""

import base64
import json


def encode(raw, padded=True, standard=False):
    """Base64 of raw bytes: URL-safe and padded unless asked otherwise."""
    text = (base64.b64encode(raw) if standard else base64.urlsafe_b64encode(raw)).decode("ascii")
    return text if padded else text.rstrip("=")


def compact(value):
    return json.dumps(value, separators=(",", ":")).encode("utf-8")


def feed_state(**changes):
    state = {"v": 1, "phase": "task", "after": "", "source": "", "event": ""}
    state.update(changes)
    return state


def stats_state(root, since, before, **changes):
    state = {"v": 1, "root": root, "since": since, "before": before, "generation": "g", "offset": 0}
    state.update(changes)
    return state


def raw_json(pairs):
    """An object written member by member, so duplicate keys and odd values survive."""
    return "{" + ",".join('"%s":%s' % (key, text) for key, text in pairs) + "}"


def padded_to(raw, size):
    """The JSON bytes with trailing blanks so that their base64 text is exactly `size` characters."""
    raw_size = size // 4 * 3
    assert len(raw) <= raw_size
    return raw + b" " * (raw_size - len(raw))


def with_char(character, field, base):
    """A state whose `field` makes the standard base64 text contain `character`."""
    for number in range(0x80, 0x3000):
        probe = dict(base, **{field: "x" + chr(number) * 3})
        text = encode(json.dumps(probe, ensure_ascii=False).encode("utf-8"), standard=True)
        if character in text:
            return probe
    raise AssertionError(character)


def feed_cases():
    """(label, cursor text) for the interaction-events cursor."""
    base = feed_state()
    ok = compact(base)
    lines = [
        ("padded", encode(ok)),
        ("unpadded", encode(ok, padded=False)),
        ("empty text", ""),
        ("space inside", encode(ok)[:8] + " " + encode(ok)[8:]),
        ("newline inside", encode(ok)[:8] + "\n" + encode(ok)[8:]),
        ("junk character", encode(ok)[:8] + "!" + encode(ok)[9:]),
        ("trailing data", encode(ok + b"x")),
        ("two documents", encode(ok + ok)),
        ("not base64 text", "not a cursor"),
        ("non-ascii text", "é" * 8),
    ]
    plus = with_char("+", "event", base)
    slash = with_char("/", "event", base)
    lines += [
        ("standard alphabet with plus", encode(json.dumps(plus, ensure_ascii=False).encode(), standard=True)),
        ("standard alphabet with slash", encode(json.dumps(slash, ensure_ascii=False).encode(), standard=True)),
        ("url alphabet with minus or underscore", encode(json.dumps(plus, ensure_ascii=False).encode())),
        ("padding missing by one", encode(compact(feed_state(event="ab")), padded=False)),
        ("padding doubled", encode(ok) + "="),
    ]
    sized = padded_to(ok, 4096)
    lines += [
        ("4096 characters", encode(sized)),
        ("4100 characters", encode(padded_to(ok, 4100))),
        ("4097 characters", "A" * 4097),
    ]
    for label, text in (
        ("v true", "true"), ("v 1.0", "1.0"), ("v 1e0", "1e0"), ("v 1.0000000000000001", "1.0000000000000001"),
        ("v 2", "2"), ("v string", '"1"'), ("v null", "null"), ("v list", "[1]"), ("v zero", "0"),
        ("v minus zero", "-0"), ("v huge", "10000000000000000000001"), ("v 1e400", "1e400"),
        ("v NaN", "NaN"), ("v Infinity", "Infinity"), ("v false", "false"),
    ):
        raw = raw_json([("v", text), ("phase", '"task"'), ("after", '""'), ("source", '""'), ("event", '""')])
        lines.append((label, encode(raw.encode())))
    for label, phase in (("phase workflow", "workflow"), ("phase x", "x"), ("phase upper", "Task"), ("phase empty", "")):
        lines.append((label, encode(compact(feed_state(phase=phase)))))
    lines.append(("phase list", encode(compact(feed_state(phase=["task"])))))
    for label, value in (
        ("after dotted", "a.b-c_d"), ("after 100", "a" * 100), ("after 101", "a" * 101), ("after underscore first", "_a"),
        ("after dash first", "-a"), ("after dot first", ".a"), ("after space", "a b"), ("after accented", "éa"),
        ("after number", 1), ("after newline end", "a\n"), ("after digit first", "9"), ("after 512", "a" * 512),
        ("after 513", "a" * 513), ("source 100", "b" * 100), ("source 101", "b" * 101), ("source slash", "a/b"),
        ("source number", 5),
    ):
        key = "source" if label.startswith("source") else "after"
        lines.append((label, encode(compact(feed_state(**{key: value})))))
    for label, value in (
        ("event 512", "e" * 512), ("event 513", "e" * 513), ("event accented 512", "é" * 512),
        ("event emoji 512", "\U0001f600" * 512), ("event emoji 513", "\U0001f600" * 513), ("event number", 5),
        ("event null", None), ("event quote and backslash", 'a"b\\c'),
    ):
        lines.append((label, encode(json.dumps(feed_state(event=value), ensure_ascii=False).encode())))
    five = ["v", "phase", "after", "source", "event"]
    lines += [
        ("list of the five keys", encode(compact(five))),
        ("extra key", encode(compact(feed_state(extra=1)))),
        ("missing key", encode(compact({k: v for k, v in base.items() if k != "event"}))),
        ("string document", encode(b'"abc"')),
        ("null document", encode(b"null")),
        ("number document", encode(b"5")),
        ("empty object", encode(b"{}")),
        ("nan event", encode(raw_json([("v", "1"), ("phase", '"task"'), ("after", '""'), ("source", '""'), ("event", "NaN")]).encode())),
        ("blank around the document", encode(b" \n" + ok + b"\t ")),
        ("raw control character in a string", encode(b'{"v":1,"phase":"task","after":"","source":"","event":"a\x01b"}')),
        ("lone surrogate escape in event", encode(raw_json([("v", "1"), ("phase", '"task"'), ("after", '""'), ("source", '""'), ("event", '"\\ud800"')]).encode())),
        ("lone surrogate escape in after", encode(raw_json([("v", "1"), ("phase", '"task"'), ("after", '"\\ud800"'), ("source", '""'), ("event", '""')]).encode())),
        ("duplicate key last valid", encode(raw_json([("v", "1"), ("phase", '"task"'), ("after", '"bad one"'), ("after", '""'), ("source", '""'), ("event", '""')]).encode())),
        ("duplicate key last invalid", encode(raw_json([("v", "1"), ("phase", '"task"'), ("after", '""'), ("after", '"bad one"'), ("source", '""'), ("event", '""')]).encode())),
        ("duplicate v last wrong", encode(raw_json([("v", "1"), ("v", "2"), ("phase", '"task"'), ("after", '""'), ("source", '""'), ("event", '""')]).encode())),
        ("utf-8 byte order mark", encode(b"\xef\xbb\xbf" + ok)),
        ("utf-16 with mark", encode(ok.decode().encode("utf-16"))),
        ("utf-16 little endian bare", encode(ok.decode().encode("utf-16-le"))),
        ("utf-16 big endian bare", encode(ok.decode().encode("utf-16-be"))),
        ("utf-32 with mark", encode(ok.decode().encode("utf-32"))),
        ("utf-32 little endian bare", encode(ok.decode().encode("utf-32-le"))),
        ("utf-32 big endian bare", encode(ok.decode().encode("utf-32-be"))),
        ("utf-16 odd length", encode(ok.decode().encode("utf-16-le") + b"x")),
        ("invalid utf-8", encode(b'{"v":1,"phase":"task","after":"","source":"","event":"\xff"}')),
        ("overlong utf-8", encode(b'{"v":1,"phase":"task","after":"","source":"","event":"\xc0\x80"}')),
        ("surrogate in utf-8", encode(b'{"v":1,"phase":"task","after":"","source":"","event":"\xed\xa0\x80"}')),
        ("second mark after the first", encode(b"\xef\xbb\xbf\xef\xbb\xbf" + ok)),
        ("deep nesting in event", encode(raw_json([("v", "1"), ("phase", '"task"'), ("after", '""'), ("source", '""'), ("event", "[" * 500 + "]" * 500)]).encode())),
    ]
    return lines


def stats_cases(root, since, before):
    """(label, cursor text) for the statistics cursor; `since` and `before` bind the window."""

    def state(**changes):
        return stats_state(root, since, before, **changes)

    def dump(value):
        return encode(compact(value))

    def pairs(**given):
        order = ["v", "root", "since", "before", "generation", "offset"]
        values = {"v": "1", "root": json.dumps(root), "since": str(since), "before": str(before),
                  "generation": '"g"', "offset": "0"}
        values.update(given)
        return raw_json([(key, values[key]) for key in order])

    lines = [
        ("plain", dump(state())),
        ("unpadded", encode(compact(state(offset=7)), padded=False)),
        ("empty text", ""),
        ("junk character", dump(state())[:6] + "!" + dump(state())[7:]),
        ("trailing data", encode(compact(state()) + b"x")),
        ("not base64 text", "not a cursor"),
        ("standard alphabet", encode(json.dumps(with_char("+", "generation", state()), ensure_ascii=False).encode(), standard=True)),
        ("url alphabet", encode(json.dumps(with_char("+", "generation", state()), ensure_ascii=False).encode())),
        ("4096 bytes", encode(padded_to(compact(state()), 4096))),
        ("4100 bytes", encode(padded_to(compact(state()), 4100))),
        ("4097 bytes of text", "A" * 4097),
        ("non-ascii text", "é" * 8),
    ]
    for label, text in (
        ("v true", "true"), ("v 1.0", "1.0"), ("v 2", "2"), ("v string", '"1"'), ("v null", "null"), ("v zero", "0"),
    ):
        lines.append((label, encode(pairs(v=text).encode())))
    for label, text in (
        ("offset true", "true"), ("offset false", "false"), ("offset minus one", "-1"), ("offset minus zero", "-0"),
        ("offset 1.5", "1.5"), ("offset 1e3", "1e3"), ("offset 2.0", "2.0"), ("offset string", '"1"'),
        ("offset null", "null"), ("offset zero", "0"), ("offset three", "3"), ("offset 2**64", "18446744073709551616"),
        ("offset 2**64 minus one", "18446744073709551615"), ("offset 10**30", "1" + "0" * 30),
        ("offset minus 2**64", "-18446744073709551616"), ("offset NaN", "NaN"), ("offset Infinity", "Infinity"),
        ("offset list", "[0]"),
    ):
        lines.append((label, encode(pairs(offset=text).encode())))
    for label, text in (
        ("generation NaN", "NaN"), ("generation number", "7"), ("generation null", "null"), ("generation list", '["g"]'),
        ("generation true", "true"), ("generation empty", '""'),
    ):
        lines.append((label, encode(pairs(generation=text).encode())))
    five = ["v", "root", "since", "before", "generation", "offset"]
    lines += [
        ("list of the six keys", encode(compact(five))),
        ("extra key", encode(compact(state(extra=1)))),
        ("missing key", encode(compact({k: v for k, v in state().items() if k != "root"}))),
        ("missing offset", encode(compact({k: v for k, v in state().items() if k != "offset"}))),
        ("string document", encode(b'"abc"')),
        ("null document", encode(b"null")),
        ("empty object", encode(b"{}")),
        ("blank around the document", encode(b" \n" + compact(state()) + b"\t ")),
        ("duplicate offset last valid", encode(raw_json([("v", "1"), ("root", json.dumps(root)), ("since", str(since)), ("before", str(before)), ("generation", '"g"'), ("offset", "-5"), ("offset", "1")]).encode())),
        ("duplicate offset last invalid", encode(raw_json([("v", "1"), ("root", json.dumps(root)), ("since", str(since)), ("before", str(before)), ("generation", '"g"'), ("offset", "1"), ("offset", "-5")]).encode())),
        ("utf-8 byte order mark", encode(b"\xef\xbb\xbf" + compact(state()))),
        ("utf-16 with mark", encode(compact(state()).decode().encode("utf-16"))),
        ("utf-16 little endian bare", encode(compact(state()).decode().encode("utf-16-le"))),
        ("utf-32 big endian bare", encode(compact(state()).decode().encode("utf-32-be"))),
        ("lone surrogate in generation", encode(pairs(generation='"\\ud800"').encode())),
        ("raw control character in generation", encode(pairs(generation='"a\x01"').encode())),
    ]
    return lines


def window_cases(root, since, before):
    """(label, cursor text, verdict key) for the binding of a statistics cursor; the cursor is
    well formed in every case, so the reference engine accepts the decoding of all of them."""

    def dump(**changes):
        value = stats_state(root, since, before)
        value.update(changes)
        return encode(compact(value))

    return [
        ("same window", dump()),
        ("another root", dump(root=root + "x")),
        ("root number", dump(root=5)),
        ("root null", dump(root=None)),
        ("since moved", dump(since=since + 1)),
        ("before moved", dump(before=before - 1)),
        ("since float of the same integer", encode(compact(stats_state(root, since, before)).replace(str(since).encode(), (str(since) + ".0").encode()))),
        ("since string", dump(since=str(since))),
        ("since null", dump(since=None)),
        ("before swapped with since", dump(since=before, before=since)),
        ("v two", dump(v=2)),
    ]
