#!/usr/bin/env python3
"""Offline JVM tests for the controlled capture/rate helper (not an APK build).

Run under scripts/buildlock.py. Reads cached Kotlin/JUnit dependencies; never
installs packages, touches phones, or runs Gradle's Rust/Android packaging tasks.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CACHE = Path.home() / '.gradle/caches/modules-2/files-2.1'


def jar(group, artifact, version=None):
    base = CACHE / group / artifact
    files = sorted(base.glob(f'{version or "*"}/*/{artifact}-*.jar'))
    if not files:
        raise SystemExit(f'Missing cached dependency: {group}:{artifact}:{version}; no download attempted')
    return str(files[-1])


def main():
    std = jar('org.jetbrains.kotlin', 'kotlin-stdlib', '2.1.0')
    junit = jar('junit', 'junit', '4.13.2')
    hamcrest = jar('org.hamcrest', 'hamcrest-core', '1.3')
    annotations = jar('org.jetbrains', 'annotations')
    compiler = ':'.join([
        jar('org.jetbrains.kotlin', 'kotlin-compiler-embeddable', '2.1.0'), std,
        jar('org.jetbrains.kotlin', 'kotlin-script-runtime', '2.1.0'),
        jar('org.jetbrains.kotlin', 'kotlin-reflect'),
        jar('org.jetbrains.intellij.deps', 'trove4j'),
        jar('org.jetbrains.kotlinx', 'kotlinx-coroutines-core-jvm'), annotations,
    ])
    tests = ROOT / 'android/app/src/test/kotlin/com/osvauld/p2p/VideoQualityTest.kt'
    source = ROOT / 'android/app/src/main/kotlin/com/osvauld/p2p/VideoQuality.kt'
    with tempfile.TemporaryDirectory(prefix='video-quality-jvm-') as output:
        cp = ':'.join([std, junit, hamcrest, annotations])
        inputs = [str(tests)] + ([str(source)] if source.exists() else [])
        subprocess.run(['java', '-Xmx1g', '-cp', compiler,
                        'org.jetbrains.kotlin.cli.jvm.K2JVMCompiler', '-no-stdlib',
                        '-no-reflect', '-jvm-target', '17', '-classpath', cp,
                        '-d', output, *inputs], check=True)
        subprocess.run(['java', '-cp', output + ':' + cp, 'org.junit.runner.JUnitCore',
                        'com.osvauld.p2p.VideoQualityTest'], check=True)


if __name__ == '__main__':
    main()
