#!/usr/bin/env python3
"""Minimal HTTP forward proxy (plain HTTP + CONNECT) for an offline target device.

Binds only to the host side of the USB point-to-point link (default 172.16.42.2)
and serves only the device's address (default 172.16.42.1), so nothing is exposed
on external interfaces and no host routing/NAT/firewall changes are required.
(postmarketOS sshd ships with AllowTcpForwarding=no, so an SSH tunnel is not used.)

Usage: scripts/host-proxy.py [bind_addr] [port] [allowed_client]
"""
import select
import socket
import socketserver
import sys
from urllib.parse import urlsplit

BUF = 65536


class Proxy(socketserver.StreamRequestHandler):
    def handle(self):
        if self.client_address[0] != ALLOWED:
            return
        line = self.rfile.readline(BUF).decode("latin-1")
        if not line:
            return
        try:
            method, target, version = line.split()
        except ValueError:
            return
        headers = []
        while True:
            h = self.rfile.readline(BUF)
            if h in (b"\r\n", b"\n", b""):
                break
            headers.append(h)

        if method == "CONNECT":
            host, _, port = target.rpartition(":")
            try:
                upstream = socket.create_connection((host, int(port)), timeout=30)
            except OSError as e:
                self.wfile.write(f"HTTP/1.1 502 Bad Gateway\r\n\r\n{e}".encode())
                return
            self.wfile.write(b"HTTP/1.1 200 Connection established\r\n\r\n")
            self.wfile.flush()
            self._pipe(upstream)
            return

        url = urlsplit(target)
        if url.scheme != "http" or not url.hostname:
            self.wfile.write(b"HTTP/1.1 400 Bad Request\r\n\r\n")
            return
        path = url.path or "/"
        if url.query:
            path += "?" + url.query
        try:
            upstream = socket.create_connection((url.hostname, url.port or 80), timeout=30)
        except OSError as e:
            self.wfile.write(f"HTTP/1.1 502 Bad Gateway\r\n\r\n{e}".encode())
            return
        req = [f"{method} {path} {version}\r\n".encode()]
        length = 0
        for h in headers:
            name, _, value = h.partition(b":")
            name = name.strip().lower()
            if name in (b"proxy-connection", b"proxy-authorization", b"connection", b"keep-alive"):
                continue
            if name == b"content-length":
                length = int(value.strip() or 0)
            req.append(h)
        # One request per connection keeps the proxy stateless.
        req.append(b"Connection: close\r\n\r\n")
        upstream.sendall(b"".join(req))
        if length:
            upstream.sendall(self.rfile.read(length))
        self._pipe(upstream)

    def _pipe(self, upstream):
        client = self.connection
        socks = [client, upstream]
        try:
            while True:
                r, _, x = select.select(socks, [], socks, 120)
                if x or not r:
                    break
                for s in r:
                    data = s.recv(BUF)
                    if not data:
                        return
                    (upstream if s is client else client).sendall(data)
        except OSError:
            pass
        finally:
            upstream.close()


class Server(socketserver.ThreadingTCPServer):
    daemon_threads = True
    allow_reuse_address = True


if __name__ == "__main__":
    bind = sys.argv[1] if len(sys.argv) > 1 else "172.16.42.2"
    port = int(sys.argv[2]) if len(sys.argv) > 2 else 3128
    ALLOWED = sys.argv[3] if len(sys.argv) > 3 else "172.16.42.1"
    with Server((bind, port), Proxy) as srv:
        print(f"host-proxy listening on {bind}:{port}, client {ALLOWED}", flush=True)
        srv.serve_forever()
