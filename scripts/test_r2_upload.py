#!/usr/bin/env python3
"""Offline publication-order tests; never calls Cloudflare."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class UploadTests(unittest.TestCase):
    def run_upload(self, signed):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repo = root / 'repo'
            metadata = repo / 'apt/dists/stable'
            metadata.mkdir(parents=True)
            if signed:
                for name in ('Release', 'Release.gpg', 'InRelease'):
                    (metadata / name).write_text('test metadata')
                (repo / 'tinline.gpg').write_text('test public key')
            binary = root / 'bin'
            binary.mkdir()
            log = root / 'aws.jsonl'
            aws = binary / 'aws'
            aws.write_text('#!/usr/bin/env python3\nimport json,os,sys\n'
                           'with open(os.environ["AWS_TEST_LOG"],"a") as f:\n'
                           ' f.write(json.dumps(sys.argv[1:])+"\\n")\n')
            aws.chmod(0o755)
            env = dict(os.environ, PATH=f'{binary}:{os.environ["PATH"]}',
                       AWS_TEST_LOG=str(log), R2_BUCKET='test-bucket',
                       R2_ENDPOINT='https://example.invalid', REPO_DIR=str(repo))
            result = subprocess.run(['bash', str(ROOT / 'scripts/upload_repo_r2.sh')],
                                    env=env, capture_output=True, text=True)
            calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
            return result, calls

    def test_unsigned_repo_never_uploads(self):
        result, calls = self.run_upload(False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, [])

    def test_order_and_retention(self):
        result, calls = self.run_upload(True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(calls), 6)
        self.assertTrue(all('--delete' not in call for call in calls))
        self.assertIn('apt/dists/*', calls[0])
        self.assertIn('*/by-hash/*', calls[1])
        for call, name in zip(calls[3:], ('Release', 'Release.gpg', 'InRelease')):
            self.assertEqual(call[1], 'cp')
            self.assertTrue(call[3].endswith('/' + name))
            self.assertIn('no-cache,max-age=0,must-revalidate', call)


if __name__ == '__main__':
    unittest.main()
