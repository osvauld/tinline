#!/usr/bin/env python3
"""Generate each app's native string files from the master list in i18n/.

    python3 scripts/i18n_gen.py           # check, then write every platform
    python3 scripts/i18n_gen.py --check   # check only (CI); exit 1 on errors
    python3 scripts/i18n_gen.py --report  # per-language coverage table

Master list: i18n/<tag>/<platform>.toml, platform = android | ios | desktop.
English (i18n/en/) is the source; other languages hold only what they translate,
and anything missing falls back to English at runtime. See i18n/README.md.
"""

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path
from xml.sax.saxutils import escape as xml_escape

ROOT = Path(__file__).resolve().parent.parent
I18N = ROOT / "i18n"
PLATFORMS = ("android", "ios", "desktop")
PLURAL_FORMS = ("zero", "one", "two", "few", "many", "other")
KEY_RE = re.compile(r"^[a-z][a-z0-9_]*$")
PLACEHOLDER_RE = re.compile(r"\{([a-z][a-z0-9_]*)\}")
# A placeholder with this name is an integer (plural count); the rest are text.
COUNT = "count"

ANDROID_RES = ROOT / "android/app/src/main/res"
IOS_CATALOG = ROOT / "ios/Tinline/Resources/Localizable.xcstrings"
DESKTOP_RS = ROOT / "crates/desktop/src/i18n/strings.rs"


def load_languages():
    langs = tomllib.loads((I18N / "languages.toml").read_text())["language"]
    if langs[0]["tag"] != "en" or not langs[0].get("source"):
        sys.exit("languages.toml: English must come first and be the source")
    return langs


def forms(entry):
    """{form: text} for one entry: {"text": s} or plural forms."""
    if "text" in entry:
        return {"text": entry["text"]}
    return {f: entry[f] for f in PLURAL_FORMS if f in entry}


def placeholders(text):
    seen = []
    for name in PLACEHOLDER_RE.findall(text):
        if name not in seen:
            seen.append(name)
    return seen


def load_platform(tag, platform):
    path = I18N / tag / f"{platform}.toml"
    if not path.exists():
        return {}
    return tomllib.loads(path.read_text())


def check(langs):
    """Return (errors, master) where master[platform][tag][key] = entry."""
    errors = []
    master = {}
    for platform in PLATFORMS:
        master[platform] = {}
        source = load_platform("en", platform)
        for key, entry in source.items():
            where = f"en/{platform}.toml [{key}]"
            if not KEY_RE.match(key):
                errors.append(f"{where}: key must be snake_case")
            if not isinstance(entry, dict):
                errors.append(f"{where}: must be a table")
                continue
            f = forms(entry)
            if "text" not in f and "other" not in f:
                errors.append(f"{where}: needs `text` or plural forms with `other`")
            if "text" not in f and COUNT not in placeholders(f.get("other", "")):
                errors.append(f"{where}: plural needs a {{{COUNT}}} placeholder")
            if platform == "ios" and "text" not in f and placeholders(f.get("other", ""))[:1] != [COUNT]:
                # trn() passes the count first and Xcode picks the plural form from argument 1.
                errors.append(f"{where}: iOS plural must start with {{{COUNT}}} (reword the English)")
            if not entry.get("note"):
                errors.append(f"{where}: needs a `note` for translators")
        master[platform]["en"] = source

        for lang in langs[1:]:
            tag = lang["tag"]
            table = load_platform(tag, platform)
            for key, entry in table.items():
                where = f"{tag}/{platform}.toml [{key}]"
                if key not in source:
                    errors.append(f"{where}: no such key in English")
                    continue
                want = set(placeholders(" ".join(forms(source[key]).values())))
                src_plural = "text" not in source[key]
                got_forms = forms(entry)
                if src_plural != ("text" not in got_forms):
                    errors.append(f"{where}: plural/text shape differs from English")
                    continue
                if src_plural and "other" not in got_forms:
                    errors.append(f"{where}: plural needs `other`")
                for form, text in got_forms.items():
                    got = set(placeholders(text))
                    # Some languages drop {count} in the `one` form ("a message").
                    if got != want and not (src_plural and got == want - {COUNT}):
                        errors.append(
                            f"{where} {form}: placeholders {sorted(got)} != English {sorted(want)}"
                        )
            master[platform][tag] = table
    return errors, master


def arg_order(entry):
    """Placeholder names in English order of first appearance."""
    return placeholders(" ".join(forms(entry).values()))


def to_positional(text, order, fmt):
    """Rewrite {name} as positional printf args; escape literal %."""
    if not order:
        return text
    text = text.replace("%", "%%")
    return PLACEHOLDER_RE.sub(lambda m: fmt(order.index(m.group(1)) + 1, m.group(1)), text)


# ---------------------------------------------------------------- Android

def android_escape(text):
    text = xml_escape(text)
    text = text.replace("\\", "\\\\").replace("'", "\\'").replace('"', '\\"')
    text = text.replace("\n", "\\n")
    if text.startswith(("@", "?")):
        text = "\\" + text
    return text


def android_fmt(pos, name):
    return f"%{pos}$d" if name == COUNT else f"%{pos}$s"


def write_android(langs, master):
    source = master["android"]["en"]
    for lang in langs:
        tag = lang["tag"]
        table = master["android"].get(tag, {})
        if not table and tag != "en":
            continue
        lines = [
            '<?xml version="1.0" encoding="utf-8"?>',
            "<!-- Generated by scripts/i18n_gen.py from i18n/. Do not edit. -->",
            "<resources>",
        ]
        for key, src in source.items():
            entry = table.get(key)
            if entry is None:
                continue
            order = arg_order(src)
            f = forms(entry)
            if tag == "en":
                lines.append(f"    <!-- {xml_escape(src['note'])} -->")
            if "text" in f:
                value = android_escape(to_positional(f["text"], order, android_fmt))
                lines.append(f'    <string name="{key}">{value}</string>')
            else:
                lines.append(f'    <plurals name="{key}">')
                for form, text in f.items():
                    value = android_escape(to_positional(text, order, android_fmt))
                    lines.append(f'        <item quantity="{form}">{value}</item>')
                lines.append("    </plurals>")
        lines.append("</resources>")
        folder = "values" if tag == "en" else "values-b+" + tag.replace("-", "+")
        out = ANDROID_RES / folder / "strings.xml"
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text("\n".join(lines) + "\n")

    shipped = [l["tag"] for l in langs if l["tag"] == "en" or master["android"].get(l["tag"])]
    config = ANDROID_RES / "xml/locales_config.xml"
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(
        '<?xml version="1.0" encoding="utf-8"?>\n'
        "<!-- Generated by scripts/i18n_gen.py. Per-app language list (Android 13+). -->\n"
        '<locale-config xmlns:android="http://schemas.android.com/apk/res/android">\n'
        + "".join(f'    <locale android:name="{t}" />\n' for t in shipped)
        + "</locale-config>\n"
    )


# ---------------------------------------------------------------- iOS

def ios_fmt(pos, name):
    return f"%{pos}$lld" if name == COUNT else f"%{pos}$@"


def write_ios(langs, master):
    source = master["ios"]["en"]
    strings = {}
    for key, src in source.items():
        order = arg_order(src)
        locs = {}
        for lang in langs:
            tag = lang["tag"]
            entry = master["ios"].get(tag, {}).get(key)
            if entry is None:
                continue
            state = "translated" if tag == "en" else "needs_review"
            f = forms(entry)
            if "text" in f:
                unit = {"state": state, "value": to_positional(f["text"], order, ios_fmt)}
                locs[tag] = {"stringUnit": unit}
            else:
                locs[tag] = {
                    "variations": {
                        "plural": {
                            form: {"stringUnit": {"state": state, "value": to_positional(t, order, ios_fmt)}}
                            for form, t in f.items()
                        }
                    }
                }
        strings[key] = {"comment": src["note"], "extractionState": "manual", "localizations": locs}
    IOS_CATALOG.parent.mkdir(parents=True, exist_ok=True)
    IOS_CATALOG.write_text(
        json.dumps({"sourceLanguage": "en", "strings": strings, "version": "1.0"},
                   ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    )


# ---------------------------------------------------------------- desktop

def rust_str(text):
    return json.dumps(text, ensure_ascii=False)


def write_desktop(langs, master):
    source = master["desktop"]["en"]
    out = [
        "// Generated by scripts/i18n_gen.py from i18n/. Do not edit.",
        "// Placeholders stay as {name}; the i18n module fills them in.",
        "",
        "#[derive(Clone, Copy, Debug)]",
        "pub enum Msg {",
        "    Text(&'static str),",
        "    /// (CLDR plural form, text); always includes \"other\".",
        "    Plural(&'static [(&'static str, &'static str)]),",
        "}",
        "",
        "/// (tag, native name, right-to-left)",
        "pub const LANGUAGES: &[(&str, &str, bool)] = &[",
    ]
    shipped = [l for l in langs if l["tag"] == "en" or master["desktop"].get(l["tag"])]
    for l in shipped:
        out.append(f"    ({rust_str(l['tag'])}, {rust_str(l['name'])}, {str(bool(l.get('rtl'))).lower()}),")
    out += ["];", "", "pub fn get(lang: &str, key: &str) -> Option<Msg> {", "    match lang {"]
    for l in shipped:
        out.append(f"        {rust_str(l['tag'])} => {l['tag'].replace('-', '_')}(key),")
    out += ["        _ => None,", "    }", "}"]
    for l in shipped:
        tag = l["tag"]
        table = master["desktop"].get(tag, {})
        out += ["", f"fn {tag.replace('-', '_')}(key: &str) -> Option<Msg> {{", "    Some(match key {"]
        for key in source:
            entry = table.get(key)
            if entry is None:
                continue
            f = forms(entry)
            if "text" in f:
                out.append(f"        {rust_str(key)} => Msg::Text({rust_str(f['text'])}),")
            else:
                pairs = ", ".join(f"({rust_str(k)}, {rust_str(v)})" for k, v in f.items())
                out.append(f"        {rust_str(key)} => Msg::Plural(&[{pairs}]),")
        out += ["        _ => return None,", "    })", "}"]
    DESKTOP_RS.parent.mkdir(parents=True, exist_ok=True)
    DESKTOP_RS.write_text("\n".join(out) + "\n")


# ---------------------------------------------------------------- main

def report(langs, master):
    print(f"{'lang':6}" + "".join(f"{p:>10}" for p in PLATFORMS))
    for lang in langs:
        row = f"{lang['tag']:6}"
        for p in PLATFORMS:
            total = len(master[p]["en"])
            done = len(master[p].get(lang["tag"], {}))
            row += f"{(f'{done}/{total}'):>10}"
        print(row)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--report", action="store_true")
    ap.add_argument("--include-rtl", action="store_true",
                    help="also ship right-to-left languages (layouts aren't RTL-ready yet)")
    args = ap.parse_args()
    langs = load_languages()
    errors, master = check(langs)
    for e in errors:
        print("error:", e, file=sys.stderr)
    if errors:
        sys.exit(1)
    if args.report:
        report(langs, master)
    if args.check or args.report:
        return
    if not args.include_rtl:
        langs = [l for l in langs if not l.get("rtl")]
    if master["android"]["en"]:
        write_android(langs, master)
    if master["ios"]["en"]:
        write_ios(langs, master)
    if master["desktop"]["en"]:
        write_desktop(langs, master)


if __name__ == "__main__":
    main()
