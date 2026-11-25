#!/usr/bin/env python3
"""Drive several `megasip --cli` instances against a local Asterisk and check behaviour."""
import json, os, queue, sqlite3, subprocess, sys, threading, time

HERE = os.path.dirname(os.path.abspath(__file__))
BIN = os.environ.get("MEGASIP_BIN", os.path.join(HERE, "../../target/debug/megasip"))
WORK = os.environ["INTEROP_WORK"]  # set by run.sh
T = os.path.join(WORK, "phones")
AST_LOG = os.path.join(WORK, "ast/log/full")


class Phone:
    def __init__(self, name):
        self.name = name
        env = dict(os.environ, MEGASIP_CONFIG_DIR=f"{T}/{name}", MEGASIP_DATA_DIR=f"{T}/{name}",
                   MEGASIP_NULL_AUDIO="1", MEGASIP_LOG=os.environ.get("MEGASIP_LOG", "info"))
        tone = {"alice": 500, "bob": 700, "carol": 900, "dave": 1100}[name]
        self.p = subprocess.Popen([BIN, "--cli", "-a", name, "--source", f"tone:{tone}"],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=open(f"{T}/{name}/err.log", "w"), text=True, env=env, bufsize=1)
        self.q = queue.Queue()
        self.events = []
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self.p.stdout:
            try:
                ev = json.loads(line)
            except Exception:
                continue
            self.events.append(ev)
            self.q.put(ev)

    def cmd(self, s):
        self.p.stdin.write(s + "\n")
        self.p.stdin.flush()

    def wait(self, pred, timeout=10, what=""):
        end = time.time() + timeout
        while time.time() < end:
            try:
                ev = self.q.get(timeout=max(0.05, end - time.time()))
            except queue.Empty:
                break
            if pred(ev):
                return ev
        raise AssertionError(f"{self.name}: timed out waiting for {what}")

    def wait_reg(self):
        return self.wait(lambda e: e["event"] == "accounts" and all(a["state"] == "registered" for a in e["accounts"]), 10, "registration")

    def wait_call(self, state, timeout=10):
        return self.wait(lambda e: e["event"] in ("call", "incoming") and e["call"]["state"] == state, timeout, f"call {state}")["call"]

    def wait_ended(self, timeout=10):
        return self.wait(lambda e: e["event"] == "ended", timeout, "ended")

    def levels(self):
        self.cmd("levels")
        return self.wait(lambda e: e["event"] == "levels", 3, "levels")

    def close(self):
        try:
            self.cmd("quit")
            self.p.wait(timeout=5)
        except Exception:
            self.p.kill()


results = []


def test(fn):
    name = fn.__name__
    try:
        fn()
        results.append((name, "PASS", ""))
        print(f"PASS {name}", flush=True)
    except AssertionError as e:
        results.append((name, "FAIL", str(e)))
        print(f"FAIL {name}: {e}", flush=True)


phones = {}


def ph(n):
    return phones[n]


def drain(*ps):
    for p in ps:
        while not p.q.empty():
            p.q.get()


def t_basic_call_and_audio():
    a, b = ph("alice"), ph("bob")
    drain(a, b)
    a.cmd("dial 1002")
    inc = b.wait(lambda e: e["event"] == "incoming", 10, "incoming")["call"]
    assert inc["number"] == "1001", inc
    assert inc["name"] == "Alice Test", inc
    a.wait_call("ringing")
    b.cmd(f"answer {inc['id']}")
    ca = a.wait_call("connected")
    cb = b.wait_call("connected")
    time.sleep(1.5)
    la, lb = a.levels(), b.levels()
    assert la["speaker"] > 0.1 and lb["speaker"] > 0.1, (la, lb)
    print(f"   codec alice={ca['codec']} bob={cb['codec']} levels a={la['speaker']:.2f} b={lb['speaker']:.2f}")
    # hold / resume
    assert abs(lb["freq"] - 500) < 60 and abs(la["freq"] - 700) < 60, (la, lb)
    a.cmd(f"hold {ca['id']} on")
    a.wait(lambda e: e["event"] == "call" and e["call"]["held"], 5, "held")
    time.sleep(1.5)
    lb = b.levels()
    assert abs(lb["freq"] - 500) > 60, f"bob still hears alice while held: {lb}"
    a.cmd(f"hold {ca['id']} off")
    a.wait(lambda e: e["event"] == "call" and not e["call"]["held"], 5, "resumed")
    time.sleep(1.5)
    lb = b.levels()
    assert lb["speaker"] > 0.1 and abs(lb["freq"] - 500) < 60, f"no alice audio after resume {lb}"
    # DTMF
    a.cmd(f"dtmf {ca['id']} 5#")
    d = b.wait(lambda e: e["event"] == "dtmf", 5, "dtmf")
    assert d["digit"] == "5", d
    b.cmd(f"hangup {cb['id']}")
    e = a.wait_ended()
    assert e["call"]["reason"] == "Remote hung up", e
    b.wait_ended()
    # Each phone keeps call history for its own account in megasip.db.
    time.sleep(0.3)
    with sqlite3.connect(f"{T}/alice/megasip.db") as db:
        rows = db.execute("SELECT number, kind FROM call_history WHERE time >= ?", (int(time.time()) - 120,)).fetchall()
    assert ("1002", "out") in rows, rows


def t_reject():
    a, b = ph("alice"), ph("bob")
    drain(a, b)
    a.cmd("dial 1002")
    inc = b.wait(lambda e: e["event"] == "incoming", 10, "incoming")["call"]
    b.cmd(f"reject {inc['id']}")
    e = b.wait_ended()
    assert e["missed"] is False, e
    e = a.wait_ended()
    assert e["call"]["reason"] in ("Declined", "Busy", "403 Forbidden"), e["call"]["reason"]


def t_cancel_is_missed():
    a, b = ph("alice"), ph("bob")
    drain(a, b)
    a.cmd("dial 1002")
    b.wait(lambda e: e["event"] == "incoming", 10, "incoming")
    a.wait_call("ringing")
    time.sleep(0.5)
    a.cmd("hangup 0")  # hang up the latest... ids differ; use hangupall
    a.cmd("hangupall")
    e = b.wait_ended()
    assert e["missed"] is True, e
    a.wait_ended()


def t_dtmf_to_asterisk():
    a = ph("alice")
    drain(a)
    a.cmd("dial 601")
    c = a.wait_call("connected")
    time.sleep(1.0)
    a.cmd(f"dtmf {c['id']} 4827")
    e = a.wait_ended(20)
    time.sleep(0.5)
    path = os.path.join(WORK, "dtmf.txt")
    got = open(path).read().strip() if os.path.exists(path) else "(nothing)"
    assert got == "DTMF_RESULT=4827", f"Asterisk received {got}"


def t_ulaw_only():
    c = ph("carol")
    drain(c)
    c.cmd("dial 600")
    call = c.wait_call("connected")
    assert call["codec"] in ("G.711 µ-law",), call["codec"]
    time.sleep(1.5)
    l = c.levels()
    assert l["speaker"] > 0.1, l
    c.cmd("hangupall")
    c.wait_ended()


def t_srtp():
    d = ph("dave")
    drain(d)
    d.cmd("dial 600")
    call = d.wait_call("connected")
    assert call["secure"], call
    time.sleep(1.5)
    l = d.levels()
    assert l["speaker"] > 0.1, f"no audio over SRTP: {l}"
    d.cmd("hangupall")
    d.wait_ended()


def t_busy_and_remote_hangup():
    a = ph("alice")
    drain(a)
    a.cmd("dial 603")
    e = a.wait_ended(10)
    assert e["call"]["reason"] == "Busy", e["call"]
    a.cmd("dial 602")
    a.wait_call("connected")
    e = a.wait_ended(10)
    assert e["call"]["reason"] == "Remote hung up", e["call"]


def t_blind_transfer():
    a, b, c = ph("alice"), ph("bob"), ph("carol")
    drain(a, b, c)
    a.cmd("dial 1002")
    inc = b.wait(lambda e: e["event"] == "incoming", 10, "incoming")["call"]
    b.cmd(f"answer {inc['id']}")
    ca = a.wait_call("connected")
    b.wait_call("connected")
    # Bob transfers Alice to Carol.
    b.cmd(f"transfer {inc['id']} 1003")
    inc_c = c.wait(lambda e: e["event"] == "incoming", 10, "carol incoming")["call"]
    c.cmd(f"answer {inc_c['id']}")
    c.wait_call("connected")
    b.wait_ended(10)
    time.sleep(1.5)
    lc = c.levels()
    assert lc["speaker"] > 0.1, f"carol hears nothing after transfer {lc}"
    c.cmd("hangupall")
    a.wait_ended(10)
    c.wait_ended()


def t_attended_transfer():
    a, b, c = ph("alice"), ph("bob"), ph("carol")
    drain(a, b, c)
    a.cmd("dial 1002")
    ib = b.wait(lambda e: e["event"] == "incoming", 10, "bob incoming")["call"]
    b.cmd(f"answer {ib['id']}")
    c1 = a.wait_call("connected")
    a.cmd("dial 1003")
    ic = c.wait(lambda e: e["event"] == "incoming", 10, "carol incoming")["call"]
    c.cmd(f"answer {ic['id']}")
    c2 = a.wait(lambda e: e["event"] == "call" and e["call"]["state"] == "connected" and e["call"]["id"] != c1["id"], 10, "second connected")["call"]
    a.cmd(f"xfer {c1['id']} {c2['id']}")
    # Alice's legs end; Bob and Carol stay connected to each other.
    a.wait_ended(10)
    a.wait_ended(10)
    time.sleep(2.0)
    lb, lc = b.levels(), c.levels()
    print(f"   after attended transfer: bob hears {lb['freq']:.0f}Hz {lb['speaker']:.2f}, carol hears {lc['freq']:.0f}Hz {lc['speaker']:.2f}")
    assert abs(lb["freq"] - 900) < 80 and abs(lc["freq"] - 700) < 80, (lb, lc)
    b.cmd("hangupall")
    c.wait_ended(10)
    b.wait_ended(10)


def t_conference():
    a, b, c = ph("alice"), ph("bob"), ph("carol")
    drain(a, b, c)
    a.cmd("dial 1002")
    ib = b.wait(lambda e: e["event"] == "incoming", 10, "bob incoming")["call"]
    b.cmd(f"answer {ib['id']}")
    a.wait_call("connected")
    a.cmd("dial 1003")
    ic = c.wait(lambda e: e["event"] == "incoming", 10, "carol incoming")["call"]
    c.cmd(f"answer {ic['id']}")
    a.wait(lambda e: e["event"] == "call" and e["call"]["state"] == "connected" and e["call"]["number"] == "1003", 10, "carol leg")
    # first call got held; conference resumes it
    a.cmd("conf on")
    b.wait(lambda e: e["event"] == "call" and not e["call"]["remoteHeld"], 6, "bob resumed")
    time.sleep(2)
    lb, lc = b.levels(), c.levels()
    assert lb["speaker"] > 0.1 and lc["speaker"] > 0.1, (lb, lc)
    a.cmd("hangupall")
    b.wait_ended(10)
    c.wait_ended(10)


if __name__ == "__main__":
    names = ["alice", "bob", "carol", "dave"]
    for n in names:
        phones[n] = Phone(n)
    try:
        for n in names:
            phones[n].wait_reg()
        print("all registered", flush=True)
        only = sys.argv[1:]
        for fn in [t_basic_call_and_audio, t_reject, t_cancel_is_missed, t_dtmf_to_asterisk, t_ulaw_only, t_srtp,
                   t_busy_and_remote_hangup, t_blind_transfer, t_attended_transfer, t_conference]:
            if not only or fn.__name__ in only:
                test(fn)
                time.sleep(0.5)
    finally:
        for p in phones.values():
            p.close()
    fails = [r for r in results if r[1] == "FAIL"]
    print(f"\n{len(results) - len(fails)}/{len(results)} passed")
    sys.exit(1 if fails else 0)
