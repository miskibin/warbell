"""Static server for the knight previs with no-cache headers, so phone refreshes
always fetch the latest index.html (plain http.server caches and serves stale)."""
import http.server, socketserver, os
os.chdir(os.path.dirname(os.path.abspath(__file__)))
class H(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header('Cache-Control', 'no-store, no-cache, must-revalidate, max-age=0')
        self.send_header('Pragma', 'no-cache')
        self.send_header('Expires', '0')
        super().end_headers()
    def log_message(self, *a): pass
with socketserver.TCPServer(('0.0.0.0', 8765), H) as s:
    s.allow_reuse_address = True
    print('serving tools/ on 0.0.0.0:8765 (no-cache)')
    s.serve_forever()
