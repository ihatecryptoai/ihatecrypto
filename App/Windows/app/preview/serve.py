"""Serves App/Windows/app for the preview: python3 serve.py <port> <dir>. A deep queue, so the logos a page asks for
all at once aren't dropped (http.server's default backlog is 5)."""
import sys, functools, http.server

class Server(http.server.ThreadingHTTPServer):
    request_queue_size = 256

handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=sys.argv[2])
handler.log_message = lambda *a: None
Server(("127.0.0.1", int(sys.argv[1])), handler).serve_forever()
