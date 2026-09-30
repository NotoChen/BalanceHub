#!/usr/bin/env python3
"""Explicit developer smoke: real CLIs, fake data, no network or payload execution.

Use Python 3.11+ on macOS arm64. Each --agent argument is an exact CLI path.
Only passing mechanisms receive evidence. A failed selected mechanism never
retains an older positive entry for the same fixture under --write-evidence.
"""

import argparse
import hashlib
import json
import os
import platform
import re
import select
import shlex
import signal
import subprocess
import tempfile
import time
import tomllib
from pathlib import Path


HERE = Path(__file__).resolve().parent
MAX_OUTPUT = 1024 * 1024
TIMEOUT = 15
NATIVE = {
    "codex": ("0.154.0", "npm", 2),
    "gemini": ("0.59.0", "npm", 2),
    "claude": ("2.1.270", "vendorNative", 3),
    "grok": ("1.0.24", "vendorNative", 2),
}


def require(value, message):
    if not value:
        raise AssertionError(message)


class Fixture:
    def __init__(self, root, executable, agent, data):
        self.root = root
        self.executable = Path(executable).resolve(strict=True)
        self.agent = agent
        self.data = data
        self.home = root / "home"
        self.workspace = root / "workspace"
        self.home.mkdir()
        self.workspace.mkdir()
        (self.workspace / ".git").mkdir()
        self.config = self.home / ("." + agent)
        self.config.mkdir()
        self.marker = root / "payload-was-executed"
        self.sentinel = root / "payload-sentinel.sh"
        self.sentinel.write_text("#!/bin/sh\n: > " + shlex.quote(str(self.marker)) + "\n")
        self.sentinel.chmod(0o700)
        self.env = {
            "HOME": str(self.home),
            "PATH": str(self.executable.parent) + ":/usr/bin:/bin:/usr/sbin:/sbin",
            "TMPDIR": str(root),
            "XDG_CONFIG_HOME": str(self.home / ".config"),
            "XDG_CACHE_HOME": str(self.home / ".cache"),
            "XDG_DATA_HOME": str(self.home / ".local/share"),
            "TERM": "dumb", "NO_COLOR": "1", "CI": "1", "LANG": "C", "LC_ALL": "C",
        }
        # npm launchers use /usr/bin/env node; bind its original installation.
        if agent in ("codex", "gemini"):
            node = Path(executable).absolute().parent / "node"
            require(node.is_file(), "selected npm launcher has no adjacent node runtime")
            self.env["PATH"] = str(node.parent) + ":/usr/bin:/bin:/usr/sbin:/sbin"
        if agent == "gemini":
            self.env["GEMINI_CLI_HOME"] = str(self.home)
        else:
            self.env[{"codex": "CODEX_HOME", "claude": "CLAUDE_CONFIG_DIR", "grok": "GROK_HOME"}[agent]] = str(self.config)
        self.profile = '(version 1) (allow default) (deny network*) (deny file-write* (require-not (subpath ' + json.dumps(str(root)) + ')))'
        self.sequence = 0

    def command(self, args):
        return ["/usr/bin/sandbox-exec", "-p", self.profile, str(self.executable), *args]

    def run(self, args, expect_success=True):
        self.sequence += 1
        stdout = self.root / f"stdout-{self.sequence}"
        stderr = self.root / f"stderr-{self.sequence}"
        with stdout.open("wb") as out, stderr.open("wb") as err:
            child = subprocess.Popen(self.command(args), cwd=self.workspace, env=self.env,
                                     stdin=subprocess.DEVNULL, stdout=out, stderr=err, start_new_session=True)
            deadline = time.monotonic() + TIMEOUT
            while child.poll() is None:
                if time.monotonic() >= deadline or stdout.stat().st_size > MAX_OUTPUT or stderr.stat().st_size > MAX_OUTPUT:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait(timeout=2)
                    raise AssertionError(f"{self.agent} bounded command timed out or exceeded output cap")
                time.sleep(0.01)
        require(not self.marker.exists(), "native management command executed a fixture payload")
        require(stdout.stat().st_size <= MAX_OUTPUT and stderr.stat().st_size <= MAX_OUTPUT, "native output exceeded cap")
        if expect_success:
            require(child.returncode == 0, f"{self.agent} {' '.join(args[:2])} failed with exit {child.returncode}")
        return child.returncode, stdout.read_text(), stderr.read_text()

    def write(self, path, value):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value, indent=2) + "\n" if isinstance(value, (dict, list)) else value)
        path.chmod(0o600)
        return path

    def skill(self, root, name="fixture-skill"):
        return self.write(root / "skills" / name / "SKILL.md", f"---\nname: {name}\ndescription: Harmless isolated smoke fixture\n---\nDo not run commands.\n")

    def assert_toml(self, path):
        return tomllib.loads(path.read_text())

    def assert_json(self, path):
        return json.loads(path.read_text())

    def version(self):
        text = self.run(["--version"])[1]
        match = re.search(r"(?<![\d.])(\d+\.\d+\.\d+)(?![\d.])", text)
        require(match and match.group(1) == NATIVE[self.agent][0], "exact selected CLI version differs from the fixture matrix")

    def app_server(self, method, params):
        self.sequence += 1
        stderr = self.root / f"app-server-stderr-{self.sequence}"
        buffer = b""
        total = 0
        deadline = time.monotonic() + TIMEOUT
        with stderr.open("wb") as err:
            child = subprocess.Popen(self.command(["app-server", "--stdio"]), cwd=self.workspace, env=self.env,
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err, start_new_session=True)
            try:
                def send(value):
                    child.stdin.write((json.dumps(value) + "\n").encode())
                    child.stdin.flush()

                def response(request_id):
                    nonlocal buffer, total
                    while time.monotonic() < deadline:
                        require(stderr.stat().st_size <= MAX_OUTPUT, "app-server stderr exceeded cap")
                        while b"\n" in buffer:
                            line, buffer = buffer.split(b"\n", 1)
                            value = json.loads(line)
                            if value.get("id") == request_id:
                                require("error" not in value, f"app-server rejected {method}")
                                return value["result"]
                        ready, _, _ = select.select([child.stdout], [], [], 0.05)
                        if ready:
                            chunk = os.read(child.stdout.fileno(), 65536)
                            require(chunk, "app-server closed before its response")
                            total += len(chunk)
                            require(total <= MAX_OUTPUT, "app-server stdout exceeded cap")
                            buffer += chunk
                    raise AssertionError("app-server response exceeded deadline")

                send({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "balancehub-native-smoke", "version": "1.0.0"}, "capabilities": {"experimentalApi": True}}})
                response(1)
                send({"method": "initialized"})
                send({"id": 2, "method": method, "params": params})
                result = response(2)
                require(not self.marker.exists(), "app-server inventory executed a fixture payload")
                return result
            finally:
                if child.stdin:
                    child.stdin.close()
                try:
                    child.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait(timeout=2)
                child.stdout.close()


def codex_mcp(fixture):
    raw = fixture.data["config"].replace("__SENTINEL__", str(fixture.sentinel))
    user = fixture.config / "config.toml"
    workspace = fixture.workspace / ".codex/config.toml"
    trust = "\n[projects." + json.dumps(str(fixture.workspace)) + ']\ntrust_level = "trusted"\n'
    for config in (user, workspace):
        fixture.write(user, trust)
        fixture.write(config, raw + (trust if config == user else ""))
        mode = config.stat().st_mode & 0o777
        for index, enabled in enumerate((False, False, True, True)):
            before = config.read_bytes()
            config.write_text(raw.replace("enabled = true", "enabled = true" if enabled else "enabled = false", 1) + (trust if config == user else ""))
            report = json.loads(fixture.run(["mcp", "get", "fixture.server", "--json"])[1])
            require(report.get("enabled") == enabled, "Codex MCP native state mismatch")
            parsed = fixture.assert_toml(config)
            require(parsed["mcp_servers"]["other"]["enabled"] and parsed["model"] == "fixture" and "# preserve comment" in config.read_text(), "Codex MCP lost unrelated config")
            require(config.stat().st_mode & 0o777 == mode, "Codex MCP changed config permissions")
            if index % 2:
                require(config.read_bytes() == before, "Codex MCP repeated toggle changed bytes")


def codex_skill(fixture):
    skills = [fixture.skill(fixture.home / ".agents"), fixture.skill(fixture.workspace / ".agents", "workspace-fixture")]
    for skill in skills:
        raw = fixture.data["config"].replace("__SKILL_PATH__", str(skill))
        config = fixture.write(fixture.config / "config.toml", raw)
        for index, enabled in enumerate((False, False, True, True)):
            before = config.read_bytes()
            config.write_text(raw.replace("enabled = true", "enabled = true" if enabled else "enabled = false"))
            report = fixture.app_server("skills/list", {"cwds": [str(fixture.workspace)], "forceReload": True})
            matches = [item for group in report["data"] for item in group["skills"] if Path(item["path"]) == skill]
            require(len(matches) == 1 and matches[0]["enabled"] == enabled, "Codex Skill native state mismatch")
            if index % 2:
                require(config.read_bytes() == before, "Codex Skill repeated toggle changed bytes")


def codex_plugin(fixture):
    plugin = fixture.config / "plugins/cache/local/fixture/1.0.0"
    manifest = {"name": "fixture", "version": "1.0.0", "description": "isolated fixture"}
    fixture.write(plugin / ".codex-plugin/plugin.json", manifest)
    fixture.skill(plugin)
    fixture.write(plugin / ".mcp.json", {"mcpServers": {"fixture-server": {"command": str(fixture.sentinel)}}})
    fixture.write(plugin / "hooks/hooks.json", {"hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": str(fixture.sentinel)}]}]}})
    marketplace = fixture.root / "marketplace"
    fixture.write(marketplace / ".agents/plugins/marketplace.json", {"name": "local", "plugins": [{"name": "fixture", "source": "./plugins/fixture"}]})
    fixture.write(marketplace / "plugins/fixture/.codex-plugin/plugin.json", manifest)
    fixture.skill(marketplace / "plugins/fixture")
    raw = fixture.data["config"] + '\n[marketplaces.local]\nsource_type = "local"\nsource = ' + json.dumps(str(marketplace)) + "\n[skills.bundled]\nenabled = false\n[features]\nplugins = true\n"
    config = fixture.write(fixture.config / "config.toml", raw)
    mode = config.stat().st_mode & 0o777
    for enabled in (False, False, True, True):
        config.write_text(raw.replace("enabled = true", "enabled = true" if enabled else "enabled = false", 1))
        native = json.loads(fixture.run(["plugin", "list", "--json"])[1])
        installed = [item for item in native["installed"] if item["pluginId"] == "fixture@local"]
        require(len(installed) == 1 and installed[0]["installed"] and installed[0]["enabled"] == enabled, "Codex Plugin installed binding or native state mismatch")
        report = fixture.app_server("skills/list", {"cwds": [str(fixture.workspace)], "forceReload": True})
        matches = [item for group in report["data"] for item in group["skills"] if item.get("pluginId") == "fixture@local" and item["name"] == "fixture:fixture-skill"]
        require(len(matches) == int(enabled) and all(item["enabled"] for item in matches), "Codex Plugin child visibility did not follow native enabled state")
        require(config.stat().st_mode & 0o777 == mode, "Codex Plugin changed config permissions")


def grok_mcp(fixture):
    user = fixture.write(fixture.config / "config.toml", fixture.data["config"].replace("__SENTINEL__", str(fixture.sentinel)))
    project = fixture.write(fixture.workspace / ".grok/config.toml", fixture.data["project"].replace("__SENTINEL__", str(fixture.sentinel)))
    modes = (user.stat().st_mode & 0o777, project.stat().st_mode & 0o777)
    for enabled in (False, False, True, True):
        fixture.run(["mcp", "enable" if enabled else "disable", "fixture-server"])
        config = fixture.assert_toml(user)
        require(config["fixture_unknown"] == "preserve" and config["mcp_servers"]["untouched"]["enabled"], "Grok MCP lost unrelated config")
        require(("fixture-server" in config.get("disabled_mcp_servers", [])) != enabled, "Grok personal MCP state mismatch")
        require(config["mcp_servers"]["fixture-server"].get("enabled", True) == enabled, "Grok MCP declaration state mismatch")
        if enabled:
            require(fixture.assert_toml(project)["mcp_servers"]["fixture-server"].get("enabled", True), "Grok project disabled entry remained sticky")
        require((user.stat().st_mode & 0o777, project.stat().st_mode & 0o777) == modes, "Grok MCP changed permissions")
        fixture.run(["mcp", "list", "--json"])


def grok_plugin(fixture):
    user = fixture.write(fixture.config / "config.toml", fixture.data["config"])
    plugin = fixture.config / "plugins/fixture-plugin"
    fixture.write(plugin / "plugin.json", fixture.data["manifest"])
    fixture.skill(plugin)
    fixture.write(plugin / "hooks/hooks.json", {"hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": str(fixture.sentinel)}]}]}})
    for enabled in (False, False, True, True):
        user.write_text(fixture.data["config"] if enabled else fixture.data["config"].replace('["fixture-plugin"]', '[]').replace('["untouched"]', '["untouched", "fixture-plugin"]'))
        config = fixture.assert_toml(user)
        require(config["fixture_unknown"] == "preserve" and "untouched" in config["plugins"]["disabled"], "Grok plugin lost unrelated config")
        listing = json.loads(fixture.run(["inspect", "--json"])[1])
        entries = listing["plugins"]
        matches = [entry for entry in entries if entry.get("name") == "fixture-plugin"]
        require(len(matches) == 1 and matches[0].get("enabled") == enabled, "Grok plugin native listing mismatch")


def grok_skill(fixture):
    user = fixture.write(fixture.config / "config.toml", fixture.data["config"])
    fixture.skill(fixture.config)
    fixture.skill(fixture.workspace / ".grok", "workspace-fixture")
    for name in ("fixture-skill", "workspace-fixture"):
        for index, enabled in enumerate((False, False, True, True)):
            before = user.read_bytes()
            user.write_text(fixture.data["config"].replace('["untouched"]', json.dumps(["untouched"] if enabled else ["untouched", name])))
            listing = json.loads(fixture.run(["inspect", "--json"])[1])
            matches = [entry for entry in listing["skills"] if entry.get("name") == name]
            require(len(matches) == 1 and matches[0].get("disabled", False) != enabled, "Grok Skill native state mismatch")
            require(fixture.assert_toml(user)["fixture_unknown"] == "preserve", "Grok Skill lost unrelated config")
            if index % 2:
                require(user.read_bytes() == before, "Grok Skill repeated toggle changed bytes")


def claude_plugin(fixture):
    plugin = fixture.config / "plugins/cache/local/fixture/1.0.0"
    fixture.write(plugin / ".claude-plugin/plugin.json", fixture.data["manifest"])
    fixture.skill(plugin)
    fixture.write(plugin / "hooks/hooks.json", {"hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": str(fixture.sentinel)}]}]}})
    paths = {"user": fixture.config / "settings.json", "project": fixture.workspace / ".claude/settings.json", "local": fixture.workspace / ".claude/settings.local.json"}
    for scope, settings in paths.items():
        for path in paths.values():
            path.unlink(missing_ok=True)
        installed = {"scope": scope, "installPath": str(plugin), "version": "1.0.0", "installedAt": "2026-09-13T00:00:00.000Z", "lastUpdated": "2026-09-13T00:00:00.000Z"}
        if scope != "user":
            installed["projectPath"] = str(fixture.workspace)
        fixture.write(fixture.config / "plugins/installed_plugins.json", {"version": 2, "plugins": {"fixture@local": [installed]}})
        fixture.write(settings, fixture.data["settings"])
        mode = settings.stat().st_mode & 0o777
        for index, enabled in enumerate((False, False, True, True)):
            before = settings.read_bytes()
            fixture.run(["plugin", "enable" if enabled else "disable", "fixture@local", "--scope", scope], expect_success=index % 2 == 0)
            config = fixture.assert_json(settings)
            require(config["enabledPlugins"]["fixture@local"] == enabled, "Claude explicit-scope state mismatch")
            require(config["enabledPlugins"]["untouched@local"] is False and config["fixtureUnknown"] == {"preserve": True}, "Claude plugin lost unrelated settings")
            if index % 2:
                require(settings.read_bytes() == before, "Claude idempotent toggle rewrote settings")
            require(all(not path.exists() for key, path in paths.items() if key != scope), "Claude wrote another scope's settings")
            require(settings.stat().st_mode & 0o777 == mode, "Claude changed settings permissions")


def gemini_mcp(fixture):
    settings = fixture.write(fixture.config / "settings.json", fixture.data["settings"])
    config = fixture.assert_json(settings)
    config["mcpServers"]["fixture-server"]["command"] = str(fixture.sentinel)
    fixture.write(settings, config)
    control = fixture.write(fixture.config / "mcp-server-enablement.json", fixture.data["enablement"])
    before = settings.read_bytes()
    for enabled in (False, False, True, True):
        fixture.run(["mcp", "enable" if enabled else "disable", "fixture-server"])
        entries = fixture.assert_json(control)
        require(entries.get("fixture-server", {}).get("enabled", True) == enabled, "Gemini global MCP state mismatch")
        require(entries["untouched"]["enabled"] is False, "Gemini MCP changed an unrelated enablement entry")
        require(settings.read_bytes() == before, "Gemini MCP changed declaration settings")


def gemini_extension(fixture):
    extension = fixture.config / "extensions/fixture-extension"
    manifest = dict(fixture.data["manifest"])
    manifest["mcpServers"] = {"fixture-child": {"command": str(fixture.sentinel)}}
    fixture.write(extension / "gemini-extension.json", manifest)
    fixture.write(fixture.config / "settings.json", fixture.data["settings"])
    control = fixture.write(fixture.config / "extensions/extension-enablement.json", fixture.data["enablement"])
    mcp = fixture.write(fixture.config / "mcp-server-enablement.json", fixture.data["mcpEnablement"])
    for scope, scope_path in [("user", fixture.home), ("workspace", fixture.workspace)]:
        for enabled in (False, False, True, True):
            fixture.run(["extensions", "enable" if enabled else "disable", "fixture-extension", "--scope", scope])
            entries = fixture.assert_json(control)
            rules = entries["fixture-extension"]["overrides"]
            expected = ("" if enabled else "!") + str(scope_path) + "/*"
            require(rules[-1] == expected, "Gemini extension explicit scope rule mismatch")
            require(entries["untouched"] == fixture.data["enablement"]["untouched"], "Gemini extension changed unrelated enablement")
            if enabled:
                require(fixture.assert_json(mcp).get("fixture-child", {}).get("enabled", True), "Gemini extension did not auto-enable its MCP")
            require(fixture.assert_json(mcp)["untouched"]["enabled"] is False, "Gemini extension changed unrelated MCP")


def gemini_skill(fixture):
    fixture.skill(fixture.config)
    fixture.skill(fixture.workspace / ".gemini", "workspace-fixture")
    user = fixture.write(fixture.config / "settings.json", fixture.data["settings"])
    workspace = fixture.write(fixture.workspace / ".gemini/settings.json", fixture.data["workspaceSettings"])
    modes = (user.stat().st_mode & 0o777, workspace.stat().st_mode & 0o777)
    for scope, name, selected, other in [("user", "fixture-skill", user, workspace), ("workspace", "workspace-fixture", workspace, user)]:
        for index, enabled in enumerate((False, False, True, True)):
            args = ["skills", "enable" if enabled else "disable", name]
            if not enabled:
                args += ["--scope", scope]
            elif index == 2:
                value = fixture.assert_json(other)
                value["skills"]["disabled"].append(name)
                fixture.write(other, value)
            before = (user.read_bytes(), workspace.read_bytes())
            fixture.run(args)
            require((name in fixture.assert_json(selected)["skills"]["disabled"]) != enabled, "Gemini Skill explicit scope state mismatch")
            if enabled:
                require(name not in fixture.assert_json(other)["skills"]["disabled"], "Gemini Skill enable did not clear the other scope")
            config = fixture.assert_json(user)
            require("untouched" in config["skills"]["disabled"] and config["fixtureUnknown"] == {"preserve": True} and "workspace-untouched" in fixture.assert_json(workspace)["skills"]["disabled"], "Gemini Skill changed unrelated config")
            require((user.stat().st_mode & 0o777, workspace.stat().st_mode & 0o777) == modes, "Gemini Skill changed settings permissions")
            if index % 2:
                require((user.read_bytes(), workspace.read_bytes()) == before, "Gemini Skill repeated toggle changed bytes")


CHECKS = {
    "codex-mcp": codex_mcp, "codex-skill": codex_skill, "codex-plugin": codex_plugin,
    "grok-mcp": grok_mcp, "grok-plugin": grok_plugin, "grok-skill": grok_skill,
    "claude-plugin": claude_plugin,
    "gemini-mcp": gemini_mcp, "gemini-extension": gemini_extension, "gemini-skill": gemini_skill,
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for agent in NATIVE:
        parser.add_argument("--" + agent, type=Path)
    parser.add_argument("--only", action="append", choices=sorted(CHECKS))
    parser.add_argument("--write-evidence", action="store_true")
    args = parser.parse_args()
    require(platform.system() == "Darwin" and platform.machine() == "arm64", "smoke evidence is limited to macOS arm64")
    evidence = []
    selected = set()
    failures = []
    for name, check in CHECKS.items():
        agent = name.split("-", 1)[0]
        executable = getattr(args, agent)
        if executable is None or (args.only and name not in args.only):
            continue
        raw = (HERE / "fixtures" / (name + ".json")).read_bytes()
        data = json.loads(raw)
        selected.add(data["mechanism"])
        try:
            with tempfile.TemporaryDirectory(prefix="balancehub-native-smoke-") as temporary:
                fixture = Fixture(Path(temporary).resolve(), executable, agent, data)
                fixture.version()
                check(fixture)
                require(not fixture.marker.exists(), "a fixture payload executed")
        except (AssertionError, ValueError, KeyError, OSError) as error:
            failures.append(name)
            print(f"FAIL {name}: {error}", flush=True)
            continue
        version, distribution, schema = NATIVE[agent]
        major, minor, patch = map(int, version.split("."))
        evidence.append({
            "id": f"{data['mechanism']}-macos-aarch64-{version}-r{data['revision']}",
            "mechanismKey": data["mechanism"],
            "fixtureDigest": hashlib.sha256(raw).hexdigest(),
            "target": {"platform": "macos", "architecture": "aarch64"},
            "distribution": distribution,
            "testedVersion": {"major": major, "minor": minor, "patch": patch, "prerelease": []},
            "adapterSchemaVersion": schema,
            "actions": ["enable", "disable"], "passed": True,
        })
        print(f"PASS {name}: {version}, isolated home, bounded output, network denied, no payload", flush=True)
    require(selected, "no selected native smoke ran")
    if args.write_evidence:
        current = json.loads((HERE / "smoke_evidence.json").read_text())
        current = [entry for entry in current if entry["mechanismKey"] not in selected]
        (HERE / "smoke_evidence.json").write_text(json.dumps(current + evidence, indent=2) + "\n")
    print(f"Verified {len(evidence)} native mechanisms")
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
