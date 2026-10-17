#!/usr/bin/env python3
"""vsock bridge for "keys in a box": the only way in or out of a wallet VM.

The VMs have no network card. Every byte crosses Firecracker's vsock, as one of:

Host side (Firecracker exposes the VM's vsock as a Unix socket `UDS`):
  --expose 127.0.0.1:8711=UDS:8711        host TCP -> VM port (n2link talks to the wallet API)
  --pinhole UDS:10001=mutinynet.com:443   VM -> host -> that one destination (allowlist = these flags)
  --relay UDS:8712=CREW_UDS:8712          VM -> another VM (organizer delivers to crew)
Guest side (inside the VM):
  --serve 8711=127.0.0.1:8711             vsock port -> local service
  --out 127.0.0.2:443=10001               local listener -> host pinhole (hostname mapped in /etc/hosts)

Firecracker protocol: host-initiated = connect(UDS) + "CONNECT <port>\\n" -> "OK <n>\\n";
guest-initiated to host port P arrives on the Unix socket "<UDS>_<P>", which we listen on.
TLS is end to end (the bridge copies bytes), so certificates are checked against the real host.
"""
from __future__ import annotations

import argparse
import asyncio
import logging
import os
import socket

log = logging.getLogger("vsock-bridge")
VMADDR_CID_HOST = 2


async def pipe(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    try:
        while data := await reader.read(65536):
            writer.write(data)
            await writer.drain()
    except (ConnectionError, OSError):
        pass
    finally:
        try:
            writer.close()
        except OSError:
            pass


async def join(a: tuple, b: tuple) -> None:
    await asyncio.gather(pipe(a[0], b[1]), pipe(b[0], a[1]))


def hostport(text: str) -> tuple[str, int]:
    host, _, port = text.rpartition(":")
    return host, int(port)


async def open_vm(uds: str, vport: int) -> tuple:
    """Host -> VM through Firecracker's Unix socket."""
    reader, writer = await asyncio.open_unix_connection(uds)
    writer.write(f"CONNECT {vport}\n".encode())
    await writer.drain()
    line = await asyncio.wait_for(reader.readline(), 10)
    if not line.startswith(b"OK "):
        writer.close()
        raise ConnectionError(f"vsock CONNECT {vport} refused: {line[:40]!r}")
    return reader, writer


async def open_vsock_host(vport: int) -> tuple:
    """Guest -> host over AF_VSOCK."""
    sock = socket.socket(socket.AF_VSOCK, socket.SOCK_STREAM)
    sock.setblocking(False)
    await asyncio.get_running_loop().sock_connect(sock, (VMADDR_CID_HOST, vport))
    return await asyncio.open_connection(sock=sock)


def serving(name: str, opener):
    async def handle(reader, writer):
        try:
            other = await opener()
        except Exception as exc:  # noqa: BLE001 - log and drop this one connection
            log.warning("%s: connect failed: %s", name, exc)
            writer.close()
            return
        log.info("%s: connection", name)
        await join((reader, writer), other)
    return handle


async def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--expose", action="append", default=[], metavar="HOST:PORT=UDS:VPORT")
    p.add_argument("--pinhole", action="append", default=[], metavar="UDS:VPORT=HOST:PORT")
    p.add_argument("--relay", action="append", default=[], metavar="UDS:VPORT=UDS2:VPORT2")
    p.add_argument("--serve", action="append", default=[], metavar="VPORT=HOST:PORT")
    p.add_argument("--out", action="append", default=[], metavar="HOST:PORT=VPORT")
    a = p.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(asctime)s vsock-bridge %(levelname)s %(message)s")
    servers = []

    for spec in a.expose:
        listen, target = spec.split("=")
        uds, vport = hostport(target)
        host, port = hostport(listen)
        servers.append(await asyncio.start_server(serving(f"expose {listen}->vm:{vport}", lambda u=uds, v=vport: open_vm(u, v)), host, port))
    for spec in a.pinhole + a.relay:
        src, dst = spec.split("=")
        uds, vport = hostport(src)
        path = f"{uds}_{vport}"
        if os.path.exists(path):
            os.unlink(path)
        if spec in a.pinhole:
            host, port = hostport(dst)
            opener = lambda h=host, p_=port: asyncio.open_connection(h, p_)  # noqa: E731
            name = f"pinhole vm:{vport}->{dst}"
        else:
            uds2, vport2 = hostport(dst)
            opener = lambda u=uds2, v=vport2: open_vm(u, v)  # noqa: E731
            name = f"relay vm:{vport}->{uds2}:{vport2}"
        servers.append(await asyncio.start_unix_server(serving(name, opener), path))
    for spec in a.serve:
        vport, target = spec.split("=")
        host, port = hostport(target)
        sock = socket.socket(socket.AF_VSOCK, socket.SOCK_STREAM)
        sock.bind((socket.VMADDR_CID_ANY, int(vport)))
        sock.listen(64)
        sock.setblocking(False)
        servers.append(await asyncio.start_server(serving(f"serve vsock:{vport}->{target}", lambda h=host, p_=port: asyncio.open_connection(h, p_)), sock=sock))
    for spec in a.out:
        listen, vport = spec.split("=")
        host, port = hostport(listen)
        servers.append(await asyncio.start_server(serving(f"out {listen}->host:{vport}", lambda v=int(vport): open_vsock_host(v)), host, port))

    if not servers:
        p.error("nothing to bridge")
    log.info("ready: %d listener(s)", len(servers))
    await asyncio.gather(*(s.serve_forever() for s in servers))


if __name__ == "__main__":
    asyncio.run(main())
