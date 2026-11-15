#!/usr/bin/env python3
"""Real local Git updates + controlled builds. Never writes a real home/bin/project."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

INSTALLER = Path(__file__).resolve().parents[1] / "install.sh"


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="greenlight-install-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.upstream = self.root / "upstream work"
        self.remote = self.root / "upstream.git"
        self.upstream.mkdir()
        self.git(self.upstream, "init", "--initial-branch=master")
        self.git(self.upstream, "config", "user.name", "Installer test")
        self.git(self.upstream, "config", "user.email", "installer-test@localhost")
        (self.upstream / "Cargo.toml").write_text('[package]\nname="greenlight"\nversion="0.1.0"\n')
        (self.upstream / "VERSION").write_text("first")
        self.commit("First")
        self.run_command("git", "clone", "--bare", str(self.upstream), str(self.remote))
        self.data = self.root / "install cache with spaces"
        self.bin = self.root / "bin $(touch SHOULD_NOT_EXIST)"
        self.project = self.root / "user project"
        (self.project / ".greenlight/state").mkdir(parents=True)
        (self.project / ".greenlight/config.yaml").write_text("personal settings\n")
        (self.project / ".greenlight/state/history.db").write_bytes(b"existing data")
        self.fake = self.root / "fake tools"
        self.fake.mkdir()
        cargo = self.fake / "cargo"
        cargo.write_text(f"#!{sys.executable}\n" + '''import json,os,pathlib,shlex,subprocess,sys
a=sys.argv[1:]
assert a[0]=='build' and '--locked' in a and '--release' in a
with open(os.environ['GREENLIGHT_TEST_CALL_LOG'],'a') as f: f.write(json.dumps(a)+'\\n')
if os.environ.get('GREENLIGHT_TEST_FAIL'): sys.exit(17)
manifest=pathlib.Path(a[a.index('--manifest-path')+1])
target=pathlib.Path(a[a.index('--target-dir')+1])/a[a.index('--target')+1]/'release'/'greenlight'
target.parent.mkdir(parents=True,exist_ok=True)
version=manifest.parent.joinpath('VERSION').read_text().strip()
value='greenlight '+version+' ('+os.environ['GREENLIGHT_BUILD_COMMIT']+')'
target.write_text("#!/bin/sh\\nprintf '%s\\\\n' "+shlex.quote(value)+"\\n"+('exit 7\\n' if os.environ.get('GREENLIGHT_TEST_BAD_BINARY') else ''))
target.chmod(0o755)
''')
        cargo.chmod(0o755)
        rustc = self.fake / "rustc"
        rustc.write_text("#!/bin/sh\nprintf 'host: x86_64-unknown-linux-gnu\\n'\n")
        rustc.chmod(0o755)
        self.env = dict(os.environ)
        self.env["PATH"] = str(self.fake) + os.pathsep + self.env["PATH"]
        self.env["GREENLIGHT_TEST_CALL_LOG"] = str(self.root / "builds.jsonl")

    def run_command(self, *args, **kwargs):
        return subprocess.run(args, text=True, capture_output=True, check=True, **kwargs)

    def git(self, directory, *args):
        return self.run_command("git", "-C", str(directory), *args).stdout.strip()

    def commit(self, message):
        self.git(self.upstream, "add", ".")
        self.git(self.upstream, "commit", "-m", message)
        return self.git(self.upstream, "rev-parse", "HEAD")

    def install(self, *args, first=False, expected=0, extra_env=None):
        command = ["bash", str(INSTALLER), "--data-dir", str(self.data)]
        if first:
            command += ["--bin-dir", str(self.bin), "--repository", str(self.remote)]
        result = subprocess.run(command + list(args), cwd=self.project, env=dict(self.env, **(extra_env or {})), text=True, capture_output=True)
        if expected == 0:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def version(self):
        return self.run_command(str(self.bin / "greenlight"), "--version").stdout

    def assert_project_unchanged(self):
        self.assertEqual((self.project / ".greenlight/config.yaml").read_text(), "personal settings\n")
        self.assertEqual((self.project / ".greenlight/state/history.db").read_bytes(), b"existing data")
        self.assertFalse((self.project / "SHOULD_NOT_EXIST").exists())
        self.assertFalse((self.data / "install.lock").exists())

    def test_first_install_repeat_and_upstream_upgrade_remember_location(self):
        self.install(first=True)
        self.assertIn("first", self.version())
        self.install()
        (self.upstream / "VERSION").write_text("second")
        revision = self.commit("Upgrade")
        self.git(self.upstream, "push", str(self.remote), "master")
        self.install()
        self.assertIn("second", self.version())
        self.assertIn(revision[:12], self.version())
        self.assertEqual((self.data / "installed-revision").read_text().strip(), revision)
        for args in map(json.loads, (self.root / "builds.jsonl").read_text().splitlines()):
            self.assertIn("--locked", args)
            self.assertEqual(args[args.index("--target") + 1], "x86_64-unknown-linux-gnu")
        self.assert_project_unchanged()

    def test_failed_build_and_invalid_executable_keep_previous_binary(self):
        self.install(first=True)
        before = (self.bin / "greenlight").read_bytes()
        (self.upstream / "VERSION").write_text("next")
        self.commit("Next")
        self.git(self.upstream, "push", str(self.remote), "master")
        for env in [{"GREENLIGHT_TEST_FAIL": "1"}, {"GREENLIGHT_TEST_BAD_BINARY": "1"}]:
            self.install(expected=1, extra_env=env)
            self.assertEqual((self.bin / "greenlight").read_bytes(), before)
            self.assert_project_unchanged()
        self.install()
        self.assertIn("next", self.version())

    def test_local_source_changes_and_origin_changes_are_not_overwritten(self):
        self.install(first=True)
        path = self.data / "source/VERSION"
        path.write_text("local custom changes")
        self.assertIn("local changes", self.install(expected=1).stderr)
        self.assertEqual(path.read_text(), "local custom changes")
        self.git(self.data / "source", "restore", "VERSION")
        self.git(self.data / "source", "remote", "set-url", "origin", "https://example.invalid/unexpected")
        self.assertIn("origin changed", self.install(expected=1).stderr)
        self.assertIn("first", self.version())
        self.assert_project_unchanged()

    def test_explicit_ref_and_automatic_updates_stay_on_selected_branch(self):
        self.git(self.upstream, "switch", "-c", "preview")
        (self.upstream / "VERSION").write_text("preview")
        self.commit("Preview")
        self.git(self.upstream, "push", str(self.remote), "preview")
        self.install("--ref", "preview", first=True)
        (self.upstream / "VERSION").write_text("preview-two")
        self.commit("Preview two")
        self.git(self.upstream, "push", str(self.remote), "preview")
        self.install()
        self.assertIn("preview-two", self.version())
        self.assertEqual((self.data / "ref").read_text().strip(), "preview")
        self.assert_project_unchanged()

    def test_bad_ref_keeps_binary_and_no_unrelated_cache_is_clobbered(self):
        self.install(first=True)
        before = (self.bin / "greenlight").read_bytes()
        self.install("--ref", "missing-branch", expected=1)
        self.assertEqual((self.bin / "greenlight").read_bytes(), before)
        shutil.rmtree(self.data / "source/.git")
        result = self.install(expected=1)
        self.assertIn("not an intact managed checkout", result.stderr)
        self.assertEqual((self.data / "source/VERSION").read_text(), "first")
        self.assert_project_unchanged()

    def test_unmanaged_data_directory_is_preserved(self):
        self.data.mkdir()
        (self.data / "personal-notes").write_text("do not overwrite")
        result = self.install(first=True, expected=1)
        self.assertIn("not empty or managed", result.stderr)
        self.assertEqual((self.data / "personal-notes").read_text(), "do not overwrite")
        self.assertFalse((self.data / "source").exists())
        self.assert_project_unchanged()


if __name__ == "__main__":
    unittest.main()
