"""Test site for OpenDM Connect: logs request headers, sets a session cookie
on the page, serves the zip as an attachment, and serves one file the way
pixeldrain does: in ranges, to at most four connections at a time."""
import json, os, re, sys, threading, time
import tempfile
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

ROOT = os.environ.get("OPENDM_E2E_DIR", os.path.join(tempfile.gettempdir(), "opendm-e2e"))
LOG = ROOT + "/requests.log"
LIMITED = "/files/limited.bin"
LIMIT = 4
active = 0
lock = threading.Lock()

class Handler(SimpleHTTPRequestHandler):
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map,
                      ".m3u8": "application/vnd.apple.mpegurl", ".ts": "video/mp2t",
                      ".webm": "video/webm", ".zip": "application/zip"}

    def log_message(self, *args):
        pass

    def end_headers(self):
        path = self.path.split("?")[0]
        if path in ("/", "/index.html"):
            self.send_header("Set-Cookie", "session=abc123; Path=/")
        if path.endswith(".zip"):
            self.send_header("Content-Disposition", 'attachment; filename="Quarterly Report.zip"')
        super().end_headers()

    def send_head(self):
        self.log_request_headers()
        return super().send_head()

    def log_request_headers(self):
        with open(LOG, "a") as log:
            log.write(json.dumps({"path": self.path, "headers": dict(self.headers), "at": time.time()}) + "\n")

    def do_GET(self):
        if self.path.split("?")[0] == LIMITED:
            return self.limited()
        super().do_GET()

    def limited(self):
        global active
        self.log_request_headers()
        with lock:
            refused = active >= LIMIT
            if not refused:
                active += 1
        if refused:
            self.send_response(403)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        try:
            path = ROOT + "/site" + LIMITED
            size = os.path.getsize(path)
            wanted = re.match(r"bytes=(\d+)-(\d*)$", self.headers.get("Range", ""))
            start = int(wanted.group(1)) if wanted else 0
            end = int(wanted.group(2)) if wanted and wanted.group(2) else size - 1
            self.send_response(206 if wanted else 200)
            self.send_header("Content-Type", "video/mp4")
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Disposition", 'attachment; filename="limited.mp4"')
            self.send_header("Content-Length", str(end - start + 1))
            if wanted:
                self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
            self.end_headers()
            with open(path, "rb") as file:
                file.seek(start)
                left = end - start + 1
                while left > 0:
                    chunk = file.read(min(65536, left))
                    self.wfile.write(chunk)
                    left -= len(chunk)
                    # Slow enough that the engine's connections overlap.
                    time.sleep(0.01)
        except (BrokenPipeError, ConnectionResetError):
            pass
        finally:
            with lock:
                active -= 1

port = int(sys.argv[1])


class Server(ThreadingHTTPServer):
    def handle_error(self, request, client_address):
        # A browser closes its own request when OpenDM takes the download over.
        if not isinstance(sys.exc_info()[1], (BrokenPipeError, ConnectionResetError)):
            super().handle_error(request, client_address)


Server(("127.0.0.1", port), partial(Handler, directory=ROOT + "/site")).serve_forever()
