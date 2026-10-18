"""Test site for OpenDM Connect: logs request headers, sets a session cookie
on the page and serves the zip as an attachment."""
import json, os, sys, time
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

ROOT = os.environ.get("OPENDM_E2E_DIR", "/tmp/opendm-e2e")
LOG = ROOT + "/requests.log"

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
        with open(LOG, "a") as log:
            log.write(json.dumps({"path": self.path, "headers": dict(self.headers), "at": time.time()}) + "\n")
        return super().send_head()

port = int(sys.argv[1])


class Server(ThreadingHTTPServer):
    def handle_error(self, request, client_address):
        # A browser closes its own request when OpenDM takes the download over.
        if not isinstance(sys.exc_info()[1], (BrokenPipeError, ConnectionResetError)):
            super().handle_error(request, client_address)


Server(("127.0.0.1", port), partial(Handler, directory=ROOT + "/site")).serve_forever()
