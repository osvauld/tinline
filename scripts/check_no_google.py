#!/usr/bin/env python3
"""Fail if an APK or AAB carries Google Play services or Firebase code (docs/push.md check W13).

    scripts/check_no_google.py android/app/build/outputs/apk/liberum/debug/app-liberum-debug.apk

Tinline Liberum ships no Google SDK. This looks for class descriptors under com/google/firebase,
com/google/android/gms and com/google/android/play in every classes*.dex (and the .class entries of a
bundle), and for their package names in the merged AndroidManifest.xml (binary XML, UTF-16 or UTF-8).
Exit 0 = clean, 1 = found, 2 = usage. Run it from CI, or by hand after `assembleLiberumRelease`.
"""
import re
import sys
import zipfile

BAD_PKGS = ("com.google.firebase", "com.google.android.gms", "com.google.android.play", "com.google.android.datatransport")
DEX = re.compile(r"(^|/)classes\d*\.dex$")


def main(argv):
    if len(argv) != 2:
        print(__doc__)
        return 2
    found = []
    with zipfile.ZipFile(argv[1]) as z:
        for name in z.namelist():
            if DEX.search(name):
                data = z.read(name)
                for needle in ("com/google/firebase", "com/google/android/gms", "com/google/android/play", "com/google/android/datatransport"):
                    n = data.count(b"L" + needle.encode() + b"/")
                    if n:
                        found.append(f"{name}: {n} class reference(s) under {needle}")
            elif name.endswith(".class") and any(name.split("/", 3)[-1].startswith(b) for b in ("com/google/firebase", "com/google/android/gms")):
                found.append(f"{name}: class file")
            elif name.endswith("AndroidManifest.xml"):
                data = z.read(name)
                for pkg in BAD_PKGS:
                    if pkg.encode() in data or pkg.encode("utf-16-le") in data:
                        found.append(f"{name}: mentions {pkg}")
    if found:
        print("Google code found:")
        for f in found:
            print("  " + f)
        return 1
    print("ok: no com.google.firebase / com.google.android.gms classes or manifest entries")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
