#!/usr/bin/env python3
"""Local WsgiDAV acceptance fixture. Uses temporary data and synthetic credentials.

Install WsgiDAV and cheroot in a venv, then run with --root and --ready-file.
The fault endpoint only exists in this loopback test process, never in the App.
"""
import argparse
import hashlib
import json
import pathlib
import threading
import time
from http import HTTPStatus

from cheroot import wsgi
from wsgidav.wsgidav_app import WsgiDAVApp
from wsgidav.fs_dav_provider import FileResource, FilesystemProvider

PREFIX = "/remote.php/dav/files/fixture"
USER = "balancehub-fixture"
PASSWORD = "balancehub-fixture-password"


class ContentEtagFile(FileResource):
    def get_etag(self):
        if self.environ.get("balancehub.coarse_etag"):
            # Exercise the default filesystem provider's second-resolution
            # timestamp/size ETags as an explicitly unsafe server case.
            return super().get_etag()
        with open(self._file_path, "rb") as stream:
            return hashlib.file_digest(stream, "sha256").hexdigest()


class ContentEtagProvider(FilesystemProvider):
    def get_resource_inst(self, path, environ):
        resource = super().get_resource_inst(path, environ)
        if isinstance(resource, FileResource):
            return ContentEtagFile(path, environ, resource._file_path)
        return resource


class Faults:
    def __init__(self, app):
        self.app = app
        self.lock = threading.Lock()
        self.rules = {}
        self.requests = []

    @staticmethod
    def respond(start_response, code, data):
        body = json.dumps(data).encode()
        start_response(f"{code} {HTTPStatus(code).phrase}", [
            ("Content-Type", "application/json"), ("Content-Length", str(len(body)))])
        return [body]

    def __call__(self, environ, start_response):
        path = environ.get("PATH_INFO", "")
        method = environ["REQUEST_METHOD"]
        if path == "/__balancehub_test__/fault" and method == "POST":
            data = json.loads(environ["wsgi.input"].read(min(4096, int(environ.get("CONTENT_LENGTH", 0)))))
            with self.lock:
                if data.get("mode"):
                    self.rules[data["prefix"]] = {**data, "count": 0}
                else:
                    self.rules.pop(data["prefix"], None)
            return self.respond(start_response, 200, {"ok": True})
        if path == "/__balancehub_test__/requests":
            with self.lock:
                requests = list(self.requests)
            return self.respond(start_response, 200, requests)
        with self.lock:
            self.requests.append({"method": method, "path": path})
            if len(self.requests) > 10000:
                del self.requests[:5000]
            rule = next((value for prefix, value in self.rules.items() if path.startswith(prefix)), {})
            mode = rule.get("mode")
            if mode == "object_failure" and method == "PUT" and "/objects/" in path:
                rule["count"] += 1
                if rule["count"] > rule.get("after", 0):
                    return self.respond(start_response, 507, {"error": "fixture storage failure"})
        if mode == "offline":
            return self.respond(start_response, 503, {"error": "fixture offline"})
        if mode == "ignore_conditions" and method == "PUT":
            environ.pop("HTTP_IF_MATCH", None)
            environ.pop("HTTP_IF_NONE_MATCH", None)
        if mode == "coarse_etag":
            environ["balancehub.coarse_etag"] = True
        if mode == "slow" and method == "GET":
            time.sleep(rule.get("seconds", 2))
        if mode == "tamper" and method == "GET" and "/objects/" in path:
            start_response("200 OK", [("ETag", '"tampered"'), ("Content-Length", "7")])
            return [b"damaged"]
        if mode == "missing_object" and method == "GET" and "/objects/" in path:
            return self.respond(start_response, 404, {"error": "fixture missing object"})
        if mode == "lost_head_ack" and method == "PUT" and path.endswith("/head.json"):
            result = self.app(environ, lambda *_args, **_kwargs: None)
            try:
                list(result)
            finally:
                if hasattr(result, "close"):
                    result.close()
            return self.respond(start_response, 503, {"error": "fixture lost acknowledgement"})
        return self.app(environ, start_response)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True)
    parser.add_argument("--ready-file", required=True)
    parser.add_argument("--port", type=int, default=0)
    args = parser.parse_args()
    root = pathlib.Path(args.root).resolve()
    root.mkdir(parents=True, exist_ok=True)
    app = WsgiDAVApp({
        "provider_mapping": {PREFIX: ContentEtagProvider(str(root))},
        "http_authenticator": {"accept_basic": True, "accept_digest": False, "default_to_digest": False},
        "simple_dc": {"user_mapping": {"*": {USER: {"password": PASSWORD}}}},
        "dir_browser": {"enable": False},
        "logging": {"enable": False}, "verbose": 0,
    })
    server = wsgi.Server(("127.0.0.1", args.port), Faults(app), numthreads=12)
    server.prepare()
    port = server.socket.getsockname()[1]
    ready = {"url": f"http://127.0.0.1:{port}{PREFIX}/", "root": str(root)}
    pathlib.Path(args.ready_file).write_text(json.dumps(ready))
    print(json.dumps(ready), flush=True)
    try:
        server.serve()
    except KeyboardInterrupt:
        pass
    finally:
        server.stop()


if __name__ == "__main__":
    main()
