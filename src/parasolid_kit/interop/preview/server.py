"""Minimal whitelist-only HTTP server for the bundled preview application."""

from __future__ import annotations

import threading
import webbrowser
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Final
from urllib.parse import urlsplit

from ...diagnostics import Diagnostic, DiagnosticKind, DiagnosticSeverity
from ..errors import PreviewError
from .writer import STATIC_ASSET_NAMES

_CONTENT_TYPES: Final = {
    "index.html": "text/html; charset=utf-8",
    "viewer.css": "text/css; charset=utf-8",
    "viewer.js": "text/javascript; charset=utf-8",
    "preview.glb": "model/gltf-binary",
    "preview.manifest.json": "application/json; charset=utf-8",
}
_ROUTES: Final = {
    "/": "index.html",
    "/index.html": "index.html",
    "/viewer.css": "viewer.css",
    "/viewer.js": "viewer.js",
    "/preview.glb": "preview.glb",
    "/preview.manifest.json": "preview.manifest.json",
}
_CSP: Final = (
    "default-src 'self'; connect-src 'self'; img-src 'self' data:; "
    "object-src 'none'; script-src 'self'; style-src 'self'; "
    "base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
)


class PreviewServer:
    """Owned server that can run blocking or in a caller-managed background thread."""

    def __init__(self, server: ThreadingHTTPServer, directory: Path) -> None:
        self._server = server
        self.directory = directory
        self._thread: threading.Thread | None = None

    @property
    def host(self) -> str:
        return str(self._server.server_address[0])

    @property
    def port(self) -> int:
        return int(self._server.server_address[1])

    @property
    def url(self) -> str:
        host = "127.0.0.1" if self.host == "0.0.0.0" else self.host
        return f"http://{host}:{self.port}/"

    def open_browser(self) -> bool:
        """Open the local URL only when explicitly called by the application."""

        return bool(webbrowser.open(self.url))

    def serve_forever(self) -> None:
        """Serve synchronously until shutdown or ``KeyboardInterrupt``."""

        self._server.serve_forever(poll_interval=0.1)

    def start(self) -> threading.Thread:
        """Start one daemon thread for tests or an embedding application."""

        if self._thread is not None and self._thread.is_alive():
            raise RuntimeError("preview server is already running")
        self._thread = threading.Thread(
            target=self.serve_forever,
            name="parasolid-kit-preview",
            daemon=True,
        )
        self._thread.start()
        return self._thread

    def close(self) -> None:
        """Stop serving and release the listening socket."""

        if self._thread is not None and self._thread.is_alive():
            self._server.shutdown()
            self._thread.join(timeout=5.0)
        self._server.server_close()

    def __enter__(self) -> PreviewServer:
        return self

    def __exit__(self, _type: object, _value: object, _traceback: object) -> None:
        self.close()


def create_preview_server(
    directory: str | Path,
    *,
    host: str = "127.0.0.1",
    port: int = 0,
    allow_external: bool = False,
) -> PreviewServer:
    """Create a server restricted to the five reviewed preview resources."""

    if not isinstance(directory, (str, Path)):
        raise TypeError("directory must be a path")
    if not isinstance(host, str) or not host.strip():
        raise ValueError("host must be a non-empty string")
    if isinstance(port, bool) or not isinstance(port, int) or not 0 <= port <= 65535:
        raise ValueError("port must be an integer between 0 and 65535")
    if not isinstance(allow_external, bool):
        raise TypeError("allow_external must be a boolean")
    host = host.strip()
    loopback = host.lower() in {"127.0.0.1", "localhost"}
    if not loopback and not allow_external:
        raise PreviewError(
            _diagnostic(
                code="preview.external_bind_forbidden",
                kind=DiagnosticKind.INVALID,
                message="external preview binding requires allow_external=True",
                details={"host": host},
            )
        )

    root = Path(directory)
    if root.is_symlink() or not root.is_dir():
        raise PreviewError(
            _diagnostic(
                code="preview.invalid_directory",
                kind=DiagnosticKind.INVALID,
                message="preview directory must be an existing non-symlink directory",
            )
        )
    required = {*STATIC_ASSET_NAMES, "preview.glb", "preview.manifest.json"}
    for name in sorted(required):
        path = root / name
        if path.is_symlink() or not path.is_file():
            raise PreviewError(
                _diagnostic(
                    code="preview.invalid_directory",
                    kind=DiagnosticKind.INVALID,
                    message=f"preview resource must be a regular non-symlink file: {name}",
                    details={"resource": name},
                )
            )

    handler = _handler(root, strict_host=loopback)
    try:
        server = ThreadingHTTPServer((host, port), handler)
    except OSError as error:
        raise PreviewError(
            _diagnostic(
                code="preview.bind_failed",
                kind=DiagnosticKind.INVALID,
                message=f"preview server could not bind {host}:{port}: {error}",
                details={"host": host, "port": port},
            )
        ) from error
    server.daemon_threads = True
    return PreviewServer(server, root)


def _handler(root: Path, *, strict_host: bool) -> type[BaseHTTPRequestHandler]:
    class Handler(BaseHTTPRequestHandler):
        server_version = "parasolid-kit-preview"
        sys_version = ""

        def do_GET(self) -> None:
            self._serve(include_body=True)

        def do_HEAD(self) -> None:
            self._serve(include_body=False)

        def do_POST(self) -> None:
            self.send_error(HTTPStatus.METHOD_NOT_ALLOWED)

        def end_headers(self) -> None:
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Security-Policy", _CSP)
            self.send_header("Cross-Origin-Resource-Policy", "same-origin")
            self.send_header("Permissions-Policy", "camera=(), microphone=(), geolocation=()")
            self.send_header("Referrer-Policy", "no-referrer")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("X-Frame-Options", "DENY")
            super().end_headers()

        def _serve(self, *, include_body: bool) -> None:
            if strict_host and not self._host_is_local():
                self.send_error(HTTPStatus.BAD_REQUEST, "invalid Host header")
                return
            route = urlsplit(self.path).path
            filename = _ROUTES.get(route)
            if filename is None:
                self.send_error(HTTPStatus.NOT_FOUND)
                return
            try:
                payload = (root / filename).read_bytes()
            except OSError:
                self.send_error(HTTPStatus.NOT_FOUND)
                return
            self.send_response(HTTPStatus.OK)
            self.send_header("Content-Type", _CONTENT_TYPES[filename])
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            if include_body:
                self.wfile.write(payload)

        def _host_is_local(self) -> bool:
            value = self.headers.get("Host", "").lower()
            allowed = {
                "127.0.0.1",
                "localhost",
                f"127.0.0.1:{self.server.server_address[1]}",
                f"localhost:{self.server.server_address[1]}",
            }
            return value in allowed

        def log_message(self, _format: str, *_arguments: object) -> None:
            return

    return Handler


def _diagnostic(
    *,
    code: str,
    kind: DiagnosticKind,
    message: str,
    details: dict[str, str | int | float | bool | None] | None = None,
) -> Diagnostic:
    return Diagnostic(
        code=code,
        severity=DiagnosticSeverity.ERROR,
        kind=kind,
        message=message,
        fatal=True,
        details={} if details is None else details,
    )


__all__ = ["PreviewServer", "create_preview_server"]
