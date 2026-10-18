#!/usr/bin/env python3
"""A slow local HTTP server for trying the app with many live downloads.

Every path serves a generated file of --size bytes with Range support,
throttled per connection. Paths under /private/ ask for a login first:
user "opendm", password "secret". Example: scripts/test-server.py --port 8765
"""
import argparse
import base64
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

parser = argparse.ArgumentParser()
parser.add_argument("--port", type=int, default=8765)
parser.add_argument("--size", type=int, default=200 * 1024 * 1024)
parser.add_argument("--rate", type=int, default=64 * 1024, help="bytes per second per connection")
args = parser.parse_args()

CHUNK = 16 * 1024
PATTERN = bytes(range(251)) * (CHUNK // 251 + 2)
LOGIN = "Basic " + base64.b64encode(b"opendm:secret").decode()


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_HEAD(self):
        self.respond(body=False)

    def do_GET(self):
        self.respond(body=True)

    def respond(self, body):
        if self.path.startswith("/private/") and self.headers.get("Authorization") != LOGIN:
            self.send_response(401)
            self.send_header("WWW-Authenticate", 'Basic realm="OpenDM test"')
            self.send_header("Content-Length", "0")
            self.send_header("Connection", "close")
            self.end_headers()
            return
        start, end = 0, args.size - 1
        ranged = self.headers.get("Range", "").startswith("bytes=")
        if ranged:
            first, _, last = self.headers["Range"][6:].partition("-")
            start = int(first or 0)
            end = min(int(last), end) if last else end
        self.send_response(206 if ranged else 200)
        if ranged:
            self.send_header("Content-Range", f"bytes {start}-{end}/{args.size}")
        self.send_header("Content-Length", str(end - start + 1))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Connection", "close")
        self.end_headers()
        if not body:
            return
        position = start
        delay = CHUNK / args.rate
        try:
            while position <= end:
                size = min(CHUNK, end - position + 1)
                offset = position % 251
                self.wfile.write(PATTERN[offset : offset + size])
                position += size
                time.sleep(delay)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def log_message(self, *_):
        pass


ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()
