#!/usr/bin/env python3
"""Translate each English phrase once, use it on every platform.

    python3 scripts/i18n_tm.py export fi   # i18n/tm/fi.toml: one entry per unique English phrase
    python3 scripts/i18n_tm.py apply fi    # fill i18n/fi/{android,ios,desktop}.toml from it
    python3 scripts/i18n_tm.py status      # translated / total per language

A tm entry (translator fills `text`, or the plural forms the language needs):

    [t_3f2a91c0]
    en = "Send"
    where = ["android chat_send: Button that sends the typed message.", ...]
    text = "Lähetä"

Plural entries carry `en_one`/`en_other` instead of `en`, and the translator
writes the language's CLDR forms (zero/one/two/few/many/other; `other` required).
When the same English means different things (the button "Call" vs the noun
"Call"), give the English entry a `context = "noun"` and it gets its own entry.
When one platform needs different wording (French "Settings" is Paramètres on
Android, Réglages on iOS), add `text_ios = "..."` (or `one_ios`, ...) to override it.
Export keeps existing translations; apply rewrites the per-platform files.
"""

import hashlib
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
I18N = ROOT / "i18n"
TM = I18N / "tm"
PLATFORMS = ("android", "ios", "desktop")
FORMS = ("zero", "one", "two", "few", "many", "other")


def english():
    """[(platform, key, entry)] from the English master files."""
    out = []
    for p in PLATFORMS:
        for key, entry in tomllib.loads((I18N / "en" / f"{p}.toml").read_text()).items():
            out.append((p, key, entry))
    return out


def tm_id(entry):
    if "text" in entry:
        src = "t\0" + entry["text"]
    else:
        src = "p\0" + "\0".join(f"{f}={entry[f]}" for f in FORMS if f in entry)
    if entry.get("context"):
        # Same English, different meaning (verb vs noun): translated separately.
        src += "\0c=" + entry["context"]
    return "t_" + hashlib.sha1(src.encode()).hexdigest()[:10]


def q(s):
    """TOML basic string."""
    out = s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\t", "\\t")
    return f'"{out}"'


def load_tm(tag):
    path = TM / f"{tag}.toml"
    return tomllib.loads(path.read_text()) if path.exists() else {}


def translated(e):
    return bool(e.get("text") or e.get("other"))


def export(tag):
    old = load_tm(tag)
    groups = {}
    for p, key, entry in english():
        g = groups.setdefault(tm_id(entry), {"entry": entry, "where": []})
        g["where"].append(f"{p} {key}: {entry.get('note', '')}")
    lines = [
        f"# Translation memory for `{tag}`. Fill `text` (or plural forms) only.",
        "# Rules: i18n/glossary/README.md, i18n/glossary/" + tag + ".md, docs/design/VOICE.md.",
        "",
    ]
    for tid, g in groups.items():
        e = g["entry"]
        lines.append(f"[{tid}]")
        if "text" in e:
            lines.append(f"en = {q(e['text'])}")
        else:
            for f in FORMS:
                if f in e:
                    lines.append(f"en_{f} = {q(e[f])}")
        lines.append("where = [")
        lines += [f"  {q(w)}," for w in g["where"]]
        lines.append("]")
        prev = old.get(tid, {})
        kept = [k for k in prev if k not in ("where",) and not k.startswith("en")]
        if kept:
            lines += [f"{k} = {q(prev[k])}" for k in kept]
        elif "text" in e:
            lines.append('text = ""')
        lines.append("")
    TM.mkdir(parents=True, exist_ok=True)
    (TM / f"{tag}.toml").write_text("\n".join(lines))
    print(f"i18n/tm/{tag}.toml: {len(groups)} phrases, {sum(translated(old.get(t, {})) for t in groups)} already translated")


def apply(tag):
    tm = load_tm(tag)
    per = {p: [] for p in PLATFORMS}
    for p, key, entry in english():
        t = tm.get(tm_id(entry))
        if not t or not translated(t):
            continue
        if "text" in entry:
            per[p].append(f"[{key}]\ntext = {q(t.get(f'text_{p}', t['text']))}\n")
        else:
            body = "".join(
                f"{f} = {q(t.get(f'{f}_{p}', t[f]))}\n" for f in FORMS if f in t
            )
            per[p].append(f"[{key}]\n{body}")
    (I18N / tag).mkdir(exist_ok=True)
    for p, items in per.items():
        head = f"# Generated from i18n/tm/{tag}.toml by scripts/i18n_tm.py apply. Edit the tm file.\n\n"
        (I18N / tag / f"{p}.toml").write_text(head + "\n".join(items))
        print(f"i18n/{tag}/{p}.toml: {len(items)} entries")


def status():
    langs = tomllib.loads((I18N / "languages.toml").read_text())["language"]
    ids = {tm_id(e) for _, _, e in english()}
    for l in langs[1:]:
        tm = load_tm(l["tag"])
        done = sum(1 for i in ids if translated(tm.get(i, {})))
        print(f"{l['tag']:5} {done}/{len(ids)}")


def main():
    if len(sys.argv) == 2 and sys.argv[1] == "status":
        return status()
    if len(sys.argv) != 3 or sys.argv[1] not in ("export", "apply"):
        sys.exit(__doc__)
    {"export": export, "apply": apply}[sys.argv[1]](sys.argv[2])


if __name__ == "__main__":
    main()
