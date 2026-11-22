#!/usr/bin/env python3
"""recepteur de test pour les webhooks de la VM : logue le corps des POST."""
import http.server
import sys

CHEMIN_LOG = sys.argv[2] if len(sys.argv) > 2 else "webhook-recus.log"


class Recepteur(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        taille = int(self.headers.get("Content-Length", 0))
        corps = self.rfile.read(taille)
        with open(CHEMIN_LOG, "ab") as f:
            f.write(corps + b"\n")
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"ok")

    def log_message(self, format, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1])
    http.server.HTTPServer(("127.0.0.1", port), Recepteur).serve_forever()
